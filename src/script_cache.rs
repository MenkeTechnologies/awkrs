//! Bytecode cache for awk scripts.
//!
//! Single-file shard at `~/.awkrs/scripts.bin`. On the 2nd+ run of a given
//! script, lex/parse/compile is skipped — the cache hit deserializes a
//! `CompiledProgram` from bincode, ready for VM execution.
//!
//! Storage layout (bincode):
//!   `ScriptShard { header, entries: HashMap<canonical_path, ScriptEntry> }`
//!   `ScriptEntry { mtime_secs, mtime_nsecs, binary_mtime_at_cache,
//!                  cached_at_secs, cp_blob: Vec<u8> }`
//!
//! Read path:
//!   - Lazy load of the shard, kept alive for the process lifetime so repeat
//!     lookups pay deserialization once.
//!   - Header validated for magic / format_version / awkrs_version / pointer_width.
//!   - Per-entry: source mtime must match, and `binary_mtime_at_cache` ≥ running
//!     awkrs binary's mtime (any rebuild of awkrs invalidates entries silently).
//!
//! Write path:
//!   - `flock(LOCK_EX)` on `scripts.bin.lock` so concurrent writers serialize.
//!   - Read existing shard, mutate, bincode-encode,
//!     write to `scripts.bin.tmp.<pid>.<nanos>`, fsync, atomic-rename.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::bytecode::CompiledProgram;

/// Magic header bytes — fail-fast if a wrong-format file is read.
pub const SHARD_MAGIC: u32 = 0x41574B52; // "AWKR"
/// Bumped on incompatible bytecode/serialization schema changes.
pub const SHARD_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShardHeader {
    pub magic: u32,
    pub format_version: u32,
    pub awkrs_version: String,
    pub pointer_width: u32,
    pub built_at_secs: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScriptEntry {
    pub mtime_secs: i64,
    pub mtime_nsecs: i64,
    pub binary_mtime_at_cache: i64,
    pub cached_at_secs: i64,
    pub cp_blob: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScriptShard {
    pub header: ShardHeader,
    pub entries: HashMap<String, ScriptEntry>,
}

/// Owned bundle handed back from `try_load` / `ScriptCache::get`.
#[derive(Debug, Clone)]
pub struct CachedScript {
    pub cp: CompiledProgram,
}

/// Shard cache keyed by canonical script path. One per shard file.
pub struct ScriptCache {
    path: PathBuf,
    lock_path: PathBuf,
    shard: Mutex<Option<ScriptShard>>,
}

#[allow(dead_code)]
impl ScriptCache {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("/tmp"));
        let lock_path = parent.join(format!(
            "{}.lock",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("scripts.bin")
        ));
        Ok(Self {
            path: path.to_path_buf(),
            lock_path,
            shard: Mutex::new(None),
        })
    }

    fn ensure_loaded(&self) {
        let mut guard = self.shard.lock();
        if guard.is_none() {
            *guard = read_shard(&self.path).filter(|s| header_ok(&s.header));
        }
    }

    fn invalidate(&self) {
        let mut guard = self.shard.lock();
        *guard = None;
    }

    /// Cache lookup. Returns `None` on miss, mtime mismatch, version drift, or
    /// awkrs-binary newer than the cached entry.
    pub fn get(&self, path: &str, mtime_secs: i64, mtime_nsecs: i64) -> Option<CachedScript> {
        self.ensure_loaded();
        let guard = self.shard.lock();
        let shard = guard.as_ref()?;
        let entry = shard.entries.get(path)?;

        if entry.mtime_secs != mtime_secs || entry.mtime_nsecs != mtime_nsecs {
            return None;
        }

        if let Some(bin_mtime) = current_binary_mtime_secs() {
            if entry.binary_mtime_at_cache < bin_mtime {
                return None;
            }
        }

        let cp: CompiledProgram = bincode::deserialize(&entry.cp_blob).ok()?;
        Some(CachedScript { cp })
    }

    /// Insert / replace an entry. Serializes the whole shard and atomic-renames.
    pub fn put(
        &self,
        path: &str,
        mtime_secs: i64,
        mtime_nsecs: i64,
        cp: &CompiledProgram,
    ) -> std::io::Result<()> {
        let cp_blob = bincode::serialize(cp)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return Ok(()),
        };

        let mut shard = match read_shard(&self.path) {
            Some(s) if header_ok(&s.header) => s,
            _ => fresh_shard(),
        };

        let bin_mtime = current_binary_mtime_secs().unwrap_or(0);
        let entry = ScriptEntry {
            mtime_secs,
            mtime_nsecs,
            binary_mtime_at_cache: bin_mtime,
            cached_at_secs: now_secs(),
            cp_blob,
        };
        shard.entries.insert(path.to_string(), entry);
        shard.header.built_at_secs = now_secs() as u64;

        write_shard_atomic(&self.path, &shard)?;
        self.invalidate();
        Ok(())
    }

    /// `(count, total_blob_bytes)` snapshot.
    pub fn stats(&self) -> (i64, i64) {
        self.ensure_loaded();
        let guard = self.shard.lock();
        let Some(shard) = guard.as_ref() else {
            return (0, 0);
        };
        let count = shard.entries.len() as i64;
        let bytes: i64 = shard.entries.values().map(|e| e.cp_blob.len() as i64).sum();
        (count, bytes)
    }

    /// Drop entries whose source file vanished or whose mtime changed.
    pub fn evict_stale(&self) -> usize {
        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return 0,
        };
        let mut shard = match read_shard(&self.path) {
            Some(s) => s,
            None => return 0,
        };
        let before = shard.entries.len();
        shard.entries.retain(|p, e| match file_mtime(Path::new(p)) {
            Some((s, ns)) => s == e.mtime_secs && ns == e.mtime_nsecs,
            None => false,
        });
        let evicted = before - shard.entries.len();
        if evicted > 0 {
            let _ = write_shard_atomic(&self.path, &shard);
            self.invalidate();
        }
        evicted
    }

    /// Delete the shard file. Idempotent.
    pub fn clear(&self) -> std::io::Result<()> {
        let _lock = acquire_lock(&self.lock_path);
        let res = match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        self.invalidate();
        res
    }
}

fn header_ok(h: &ShardHeader) -> bool {
    h.magic == SHARD_MAGIC
        && h.format_version == SHARD_FORMAT_VERSION
        && h.pointer_width as usize == std::mem::size_of::<usize>()
        && h.awkrs_version == env!("CARGO_PKG_VERSION")
}

fn fresh_shard() -> ScriptShard {
    ScriptShard {
        header: ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION,
            awkrs_version: env!("CARGO_PKG_VERSION").to_string(),
            pointer_width: std::mem::size_of::<usize>() as u32,
            built_at_secs: now_secs() as u64,
        },
        entries: HashMap::new(),
    }
}

fn read_shard(path: &Path) -> Option<ScriptShard> {
    let bytes = std::fs::read(path).ok()?;
    bincode::deserialize(&bytes).ok()
}

fn acquire_lock(path: &Path) -> Option<nix::fcntl::Flock<File>> {
    let f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    nix::fcntl::Flock::lock(f, nix::fcntl::FlockArg::LockExclusive).ok()
}

fn write_shard_atomic(path: &Path, shard: &ScriptShard) -> std::io::Result<()> {
    let bytes = bincode::serialize(shard)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let parent = path.parent().expect("cache path has parent");
    let _ = std::fs::create_dir_all(parent);
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(
        "{}.tmp.{}.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("scripts.bin"),
        pid,
        nanos
    ));
    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Get mtime from file metadata as `(secs, nsecs)`.
pub fn file_mtime(path: &Path) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.mtime(), meta.mtime_nsec()))
}

/// Mtime of the running awkrs binary. Cached for the lifetime of the process.
fn current_binary_mtime_secs() -> Option<i64> {
    static BIN_MTIME: OnceLock<Option<i64>> = OnceLock::new();
    *BIN_MTIME.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let (secs, _) = file_mtime(&exe)?;
        Some(secs)
    })
}

/// Default shard path: `~/.awkrs/scripts.bin`.
pub fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".awkrs/scripts.bin")
}

/// `AWKRS_CACHE=0|false|no` disables the cache entirely.
pub fn cache_enabled() -> bool {
    !matches!(
        std::env::var("AWKRS_CACHE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    )
}

/// Process-wide `ScriptCache` rooted at `default_cache_path()`. `None` when the
/// cache is disabled or the path could not be opened.
pub static CACHE: once_cell::sync::Lazy<Option<ScriptCache>> = once_cell::sync::Lazy::new(|| {
    if !cache_enabled() {
        return None;
    }
    ScriptCache::open(&default_cache_path()).ok()
});

/// Try to load a cached `CompiledProgram` for the given source script path.
/// Returns `None` on any miss.
pub fn try_load(path: &Path) -> Option<CompiledProgram> {
    let cache = CACHE.as_ref()?;
    let canonical = path.canonicalize().ok()?;
    let path_str = canonical.to_string_lossy();
    let (mtime_s, mtime_ns) = file_mtime(&canonical)?;
    cache.get(&path_str, mtime_s, mtime_ns).map(|c| c.cp)
}

/// Store a compiled script in the cache. Silently no-ops on any failure
/// (cache disabled, path can't be canonicalized, write fails).
pub fn try_save(path: &Path, cp: &CompiledProgram) {
    let Some(cache) = CACHE.as_ref() else {
        return;
    };
    let Ok(canonical) = path.canonicalize() else {
        return;
    };
    let path_str = canonical.to_string_lossy();
    let Some((mtime_s, mtime_ns)) = file_mtime(&canonical) else {
        return;
    };
    let _ = cache.put(&path_str, mtime_s, mtime_ns, cp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::parser::parse_program;
    use tempfile::tempdir;

    fn compile(src: &str) -> CompiledProgram {
        Compiler::compile_program(&parse_program(src).unwrap()).unwrap()
    }

    #[test]
    fn round_trip() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.bin");
        let cache = ScriptCache::open(&cache_path).unwrap();

        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { print 42 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();
        let cp = compile("BEGIN { print 42 }");
        cache
            .put(&script_path.to_string_lossy(), s, ns, &cp)
            .unwrap();

        let loaded = cache.get(&script_path.to_string_lossy(), s, ns).unwrap();
        assert_eq!(loaded.cp.begin_chunks.len(), cp.begin_chunks.len());
        assert_eq!(loaded.cp.slot_count, cp.slot_count);
    }

    #[test]
    fn mtime_invalidation() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.bin");
        let cache = ScriptCache::open(&cache_path).unwrap();

        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { print 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();
        let cp = compile("BEGIN { print 1 }");
        cache
            .put(&script_path.to_string_lossy(), s, ns, &cp)
            .unwrap();
        assert!(cache
            .get(&script_path.to_string_lossy(), s + 1, ns)
            .is_none());
    }

    #[test]
    fn second_put_adds_entry() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.bin");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p1 = dir.path().join("a.awk");
        let p2 = dir.path().join("b.awk");
        std::fs::write(&p1, "BEGIN { print 1 }").unwrap();
        std::fs::write(&p2, "BEGIN { print 2 }").unwrap();
        let (s1, n1) = file_mtime(&p1).unwrap();
        let (s2, n2) = file_mtime(&p2).unwrap();
        cache
            .put(&p1.to_string_lossy(), s1, n1, &compile("BEGIN { print 1 }"))
            .unwrap();
        cache
            .put(&p2.to_string_lossy(), s2, n2, &compile("BEGIN { print 2 }"))
            .unwrap();
        let (count, _) = cache.stats();
        assert_eq!(count, 2);
        assert!(cache.get(&p1.to_string_lossy(), s1, n1).is_some());
        assert!(cache.get(&p2.to_string_lossy(), s2, n2).is_some());
    }

    #[test]
    fn corrupt_file_returns_none() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.bin");
        std::fs::write(&cache_path, b"garbage not a real shard").unwrap();
        let cache = ScriptCache::open(&cache_path).unwrap();
        assert!(cache.get("/nope", 0, 0).is_none());
    }

    #[test]
    fn clear_removes_file() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.bin");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();
        cache
            .put(
                &script_path.to_string_lossy(),
                s,
                ns,
                &compile("BEGIN { 1 }"),
            )
            .unwrap();
        assert!(cache_path.exists());
        cache.clear().unwrap();
        assert!(!cache_path.exists());
    }
}
