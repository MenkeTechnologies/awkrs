//! rkyv-backed bytecode cache for awk scripts.
//!
//! Single-file shard at `~/.awkrs/scripts.rkyv`. On the 2nd+ run of a given
//! script, lex/parse/compile is skipped — the cache hit is `mmap` + zero-copy
//! `ArchivedHashMap` lookup + bincode-decode of the inner `CompiledProgram`
//! blob. Same architecture as `strykelang/script_cache.rs`.
//!
//! Storage layout (rkyv archived):
//!   `ScriptShard { header: { magic, format_version, awkrs_version,
//!                            pointer_width, built_at_secs },
//!                  entries: HashMap<canonical_path, ScriptEntry> }`
//!   `ScriptEntry { mtime_secs, mtime_nsecs, binary_mtime_at_cache,
//!                  cached_at_secs, cp_blob: Vec<u8> }`
//!
//! Inner `cp_blob` stays bincode for now — `CompiledProgram` has no
//! Arc-shared graph that defeats rkyv's `Archive` derive, so a phase-2 pass
//! could derive `Archive` directly on `CompiledProgram` for true zero-copy
//! load. Phase 1 matches strykelang's outer-rkyv / inner-bincode split.
//!
//! Read path:
//!   - Lazy `mmap` of the shard, kept alive for the process lifetime so repeat
//!     lookups (e.g. a script that calls into the cache N times across forks)
//!     pay validation once.
//!   - `rkyv::check_archived_root::<ScriptShard>` validates the byte image.
//!   - Header validated for magic / format_version / awkrs_version / pointer_width.
//!   - Per-entry: source mtime must match, and `binary_mtime_at_cache` ≥ running
//!     awkrs binary's mtime (any rebuild invalidates entries silently).
//!
//! Write path:
//!   - `flock(LOCK_EX)` on `scripts.rkyv.lock` so concurrent writers serialize.
//!   - Read existing shard into owned form, mutate, `rkyv::to_bytes`,
//!     write to `scripts.rkyv.tmp.<pid>.<nanos>`, fsync, atomic-rename.
//!   - Drop the in-process `mmap` so the next read picks up the new shard.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use memmap2::Mmap;
use parking_lot::Mutex;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

use crate::bytecode::CompiledProgram;

/// Magic header bytes — fail-fast if a wrong-format file is mmap'd.
pub const SHARD_MAGIC: u32 = 0x41574B52; // "AWKR"
/// Bumped on incompatible rkyv schema changes.
pub const SHARD_FORMAT_VERSION: u32 = 2;

// ── rkyv archived types ──────────────────────────────────────────────────────

#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ShardHeader {
    pub magic: u32,
    pub format_version: u32,
    pub awkrs_version: String,
    pub pointer_width: u32,
    pub built_at_secs: u64,
}

#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ScriptEntry {
    pub mtime_secs: i64,
    pub mtime_nsecs: i64,
    pub binary_mtime_at_cache: i64,
    pub cached_at_secs: i64,
    pub cp_blob: Vec<u8>,
}

#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ScriptShard {
    pub header: ShardHeader,
    pub entries: HashMap<String, ScriptEntry>,
}

/// Owned bundle handed back from `try_load` / `ScriptCache::get`.
#[derive(Debug, Clone)]
pub struct CachedScript {
    pub cp: CompiledProgram,
}

// ── mmap'd validated shard view ──────────────────────────────────────────────

/// mmap + validated `*const ArchivedScriptShard`. Self-referential — the
/// pointer is valid for the lifetime of the wrapping struct.
pub struct MmappedShard {
    _mmap: Mmap,
    archived: *const ArchivedScriptShard,
}

// SAFETY: the pointer aliases an immutable mmap that lives as long as Self.
// rkyv-validated reads are immutable.
unsafe impl Send for MmappedShard {}
unsafe impl Sync for MmappedShard {}

impl MmappedShard {
    pub fn open(path: &Path) -> Option<Self> {
        let file = File::open(path).ok()?;
        let mmap = unsafe { Mmap::map(&file).ok()? };
        let archived = rkyv::check_archived_root::<ScriptShard>(&mmap[..]).ok()?;
        let archived_ptr = archived as *const ArchivedScriptShard;
        Some(Self {
            _mmap: mmap,
            archived: archived_ptr,
        })
    }

    fn shard(&self) -> &ArchivedScriptShard {
        // SAFETY: see Self impl comment.
        unsafe { &*self.archived }
    }

    fn header_ok(&self) -> bool {
        let h = &self.shard().header;
        let magic: u32 = h.magic.into();
        let fv: u32 = h.format_version.into();
        let pw: u32 = h.pointer_width.into();
        magic == SHARD_MAGIC
            && fv == SHARD_FORMAT_VERSION
            && pw as usize == std::mem::size_of::<usize>()
            && h.awkrs_version.as_str() == env!("CARGO_PKG_VERSION")
    }

    fn lookup(&self, path: &str) -> Option<&ArchivedScriptEntry> {
        self.shard().entries.get(path)
    }

    fn entry_count(&self) -> usize {
        self.shard().entries.len()
    }
}

// ── ScriptCache: per-instance handle ─────────────────────────────────────────

pub struct ScriptCache {
    path: PathBuf,
    lock_path: PathBuf,
    mmap: Mutex<Option<MmappedShard>>,
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
                .unwrap_or("scripts.rkyv")
        ));
        Ok(Self {
            path: path.to_path_buf(),
            lock_path,
            mmap: Mutex::new(None),
        })
    }

    fn ensure_mmap(&self) {
        let mut guard = self.mmap.lock();
        if guard.is_none() {
            *guard = MmappedShard::open(&self.path);
        }
    }

    fn invalidate_mmap(&self) {
        let mut guard = self.mmap.lock();
        *guard = None;
    }

    /// Cache lookup. Returns `None` on miss, mtime mismatch, version drift, or
    /// awkrs-binary newer than the cached entry.
    pub fn get(&self, path: &str, mtime_secs: i64, mtime_nsecs: i64) -> Option<CachedScript> {
        self.ensure_mmap();
        let guard = self.mmap.lock();
        let shard = guard.as_ref()?;
        if !shard.header_ok() {
            return None;
        }
        let entry = shard.lookup(path)?;

        let entry_mtime_s: i64 = entry.mtime_secs.into();
        let entry_mtime_ns: i64 = entry.mtime_nsecs.into();
        if entry_mtime_s != mtime_secs || entry_mtime_ns != mtime_nsecs {
            return None;
        }

        if let Some(bin_mtime) = current_binary_mtime_secs() {
            let cached_bin_mtime: i64 = entry.binary_mtime_at_cache.into();
            if cached_bin_mtime < bin_mtime {
                return None;
            }
        }

        let cp_bytes: &[u8] = entry.cp_blob.as_slice();
        let cp: CompiledProgram = bincode::deserialize(cp_bytes).ok()?;
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
        let cp_blob = bincode::serialize(cp).map_err(|e| std::io::Error::other(e.to_string()))?;

        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return Ok(()),
        };

        let mut shard = match read_owned_shard(&self.path) {
            Some(s)
                if s.header.awkrs_version == env!("CARGO_PKG_VERSION")
                    && s.header.pointer_width as usize == std::mem::size_of::<usize>()
                    && s.header.format_version == SHARD_FORMAT_VERSION =>
            {
                s
            }
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
        self.invalidate_mmap();
        Ok(())
    }

    /// `(count, total_blob_bytes)` snapshot.
    pub fn stats(&self) -> (i64, i64) {
        self.ensure_mmap();
        let guard = self.mmap.lock();
        let Some(shard) = guard.as_ref() else {
            return (0, 0);
        };
        let count = shard.entry_count() as i64;
        let bytes: i64 = shard
            .shard()
            .entries
            .values()
            .map(|e| e.cp_blob.len() as i64)
            .sum();
        (count, bytes)
    }

    /// Drop entries whose source file vanished or whose mtime changed.
    pub fn evict_stale(&self) -> usize {
        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return 0,
        };
        let mut shard = match read_owned_shard(&self.path) {
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
            self.invalidate_mmap();
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
        self.invalidate_mmap();
        res
    }
}

// ── Locking + shard read/write helpers ───────────────────────────────────────

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

fn read_owned_shard(path: &Path) -> Option<ScriptShard> {
    let bytes = std::fs::read(path).ok()?;
    let archived = rkyv::check_archived_root::<ScriptShard>(&bytes[..]).ok()?;
    archived.deserialize(&mut rkyv::Infallible).ok()
}

fn write_shard_atomic(path: &Path, shard: &ScriptShard) -> std::io::Result<()> {
    let bytes = rkyv::to_bytes::<_, 4096>(shard)
        .map_err(|e| std::io::Error::other(format!("rkyv serialize: {}", e)))?;

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
            .unwrap_or("scripts.rkyv"),
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

/// Default shard path: `~/.awkrs/scripts.rkyv`.
pub fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".awkrs/scripts.rkyv")
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
pub fn try_load(path: &Path) -> Option<CompiledProgram> {
    let cache = CACHE.as_ref()?;
    let canonical = path.canonicalize().ok()?;
    let path_str = canonical.to_string_lossy();
    let (mtime_s, mtime_ns) = file_mtime(&canonical)?;
    cache.get(&path_str, mtime_s, mtime_ns).map(|c| c.cp)
}

/// Store a compiled script in the cache. Silently no-ops on any failure.
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
        let cache_path = dir.path().join("scripts.rkyv");
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
        let cache_path = dir.path().join("scripts.rkyv");
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
        let cache_path = dir.path().join("scripts.rkyv");
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
        let cache_path = dir.path().join("scripts.rkyv");
        std::fs::write(&cache_path, b"garbage not a real rkyv archive").unwrap();
        let cache = ScriptCache::open(&cache_path).unwrap();
        assert!(cache.get("/nope", 0, 0).is_none());
    }

    #[test]
    fn clear_removes_file() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
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

    // ── Header-drift tests: each writes a hand-crafted shard with one wrong
    // header field and verifies the cache treats every entry inside as a miss.
    // These guard against silent acceptance of cross-version / cross-arch shards
    // that would feed mismatched bytecode into the VM.

    /// Build a shard with arbitrary header + one entry, write it through the
    /// atomic-rename path, and return the path it was written to.
    fn write_shard_with_header(
        dir: &std::path::Path,
        header: ShardHeader,
        script_path: &str,
        mtime_s: i64,
        mtime_ns: i64,
        bin_mtime: i64,
        cp: &CompiledProgram,
    ) -> PathBuf {
        let cache_path = dir.join("scripts.rkyv");
        let mut entries = HashMap::new();
        entries.insert(
            script_path.to_string(),
            ScriptEntry {
                mtime_secs: mtime_s,
                mtime_nsecs: mtime_ns,
                binary_mtime_at_cache: bin_mtime,
                cached_at_secs: 0,
                cp_blob: bincode::serialize(cp).unwrap(),
            },
        );
        let shard = ScriptShard { header, entries };
        write_shard_atomic(&cache_path, &shard).unwrap();
        cache_path
    }

    #[test]
    fn format_version_drift_rejects() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();

        // Header with bumped format_version — cache MUST reject every entry.
        let header = ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION + 1,
            awkrs_version: env!("CARGO_PKG_VERSION").to_string(),
            pointer_width: std::mem::size_of::<usize>() as u32,
            built_at_secs: 0,
        };
        let cache_path = write_shard_with_header(
            dir.path(),
            header,
            &script_path.to_string_lossy(),
            s,
            ns,
            i64::MAX, // future-dated bin_mtime so the bin-mtime check can't fail first
            &compile("BEGIN { 1 }"),
        );
        let cache = ScriptCache::open(&cache_path).unwrap();
        assert!(
            cache.get(&script_path.to_string_lossy(), s, ns).is_none(),
            "format_version drift must invalidate cached entries"
        );
    }

    #[test]
    fn awkrs_version_drift_rejects() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();

        let header = ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION,
            awkrs_version: "999.999.999".to_string(),
            pointer_width: std::mem::size_of::<usize>() as u32,
            built_at_secs: 0,
        };
        let cache_path = write_shard_with_header(
            dir.path(),
            header,
            &script_path.to_string_lossy(),
            s,
            ns,
            i64::MAX,
            &compile("BEGIN { 1 }"),
        );
        let cache = ScriptCache::open(&cache_path).unwrap();
        assert!(
            cache.get(&script_path.to_string_lossy(), s, ns).is_none(),
            "awkrs_version drift must invalidate cached entries"
        );
    }

    #[test]
    fn pointer_width_drift_rejects() {
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();

        let wrong_width = if std::mem::size_of::<usize>() == 8 {
            4
        } else {
            8
        };
        let header = ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION,
            awkrs_version: env!("CARGO_PKG_VERSION").to_string(),
            pointer_width: wrong_width as u32,
            built_at_secs: 0,
        };
        let cache_path = write_shard_with_header(
            dir.path(),
            header,
            &script_path.to_string_lossy(),
            s,
            ns,
            i64::MAX,
            &compile("BEGIN { 1 }"),
        );
        let cache = ScriptCache::open(&cache_path).unwrap();
        assert!(
            cache.get(&script_path.to_string_lossy(), s, ns).is_none(),
            "pointer_width drift must invalidate cached entries"
        );
    }

    #[test]
    fn binary_mtime_invalidation_rejects() {
        // Entry stamped with a binary mtime far in the past — the running
        // binary's mtime will be newer, so the entry must be invalidated.
        let dir = tempdir().unwrap();
        let script_path = dir.path().join("t.awk");
        std::fs::write(&script_path, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&script_path).unwrap();

        let header = ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION,
            awkrs_version: env!("CARGO_PKG_VERSION").to_string(),
            pointer_width: std::mem::size_of::<usize>() as u32,
            built_at_secs: 0,
        };
        let cache_path = write_shard_with_header(
            dir.path(),
            header,
            &script_path.to_string_lossy(),
            s,
            ns,
            0, // entry stamped 1970 — binary is newer (mtime > 0)
            &compile("BEGIN { 1 }"),
        );
        let cache = ScriptCache::open(&cache_path).unwrap();
        // Only meaningful if the binary actually has a positive mtime; on every
        // real system the awkrs test binary has been written, so its mtime > 0.
        if current_binary_mtime_secs().unwrap_or(0) > 0 {
            assert!(
                cache.get(&script_path.to_string_lossy(), s, ns).is_none(),
                "entry with bin_mtime_at_cache < running binary mtime must miss"
            );
        }
    }

    // ── evict_stale tests ────────────────────────────────────────────────────

    #[test]
    fn evict_stale_removes_vanished_sources() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();

        let p1 = dir.path().join("keep.awk");
        let p2 = dir.path().join("vanish.awk");
        std::fs::write(&p1, "BEGIN { 1 }").unwrap();
        std::fs::write(&p2, "BEGIN { 2 }").unwrap();
        let (s1, n1) = file_mtime(&p1).unwrap();
        let (s2, n2) = file_mtime(&p2).unwrap();
        cache
            .put(&p1.to_string_lossy(), s1, n1, &compile("BEGIN { 1 }"))
            .unwrap();
        cache
            .put(&p2.to_string_lossy(), s2, n2, &compile("BEGIN { 2 }"))
            .unwrap();

        // Delete one source file out from under the cache.
        std::fs::remove_file(&p2).unwrap();

        let evicted = cache.evict_stale();
        assert_eq!(evicted, 1, "vanished source must be evicted");
        assert!(cache.get(&p1.to_string_lossy(), s1, n1).is_some());
        assert!(cache.get(&p2.to_string_lossy(), s2, n2).is_none());
    }

    #[test]
    fn evict_stale_removes_mtime_changed() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();

        let p = dir.path().join("t.awk");
        std::fs::write(&p, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&p).unwrap();
        cache
            .put(&p.to_string_lossy(), s, ns, &compile("BEGIN { 1 }"))
            .unwrap();

        // Bump the source file's mtime by rewriting (filesystem grants new mtime).
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&p, "BEGIN { 2 }").unwrap();
        let (s2, _ns2) = file_mtime(&p).unwrap();
        assert!(s2 >= s, "rewrite did not advance mtime — fs precision?");

        let evicted = cache.evict_stale();
        assert_eq!(evicted, 1, "mtime-changed source must be evicted");
    }

    #[test]
    fn evict_stale_returns_zero_when_clean() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = dir.path().join("t.awk");
        std::fs::write(&p, "BEGIN { 1 }").unwrap();
        let (s, ns) = file_mtime(&p).unwrap();
        cache
            .put(&p.to_string_lossy(), s, ns, &compile("BEGIN { 1 }"))
            .unwrap();
        assert_eq!(cache.evict_stale(), 0);
    }

    // ── stats() + multi-entry behavior ───────────────────────────────────────

    #[test]
    fn stats_sums_blob_bytes_across_entries() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();

        // Three distinct scripts so the entries are independent.
        for (i, src) in [
            "BEGIN { 1 }",
            "BEGIN { print 2 }",
            "BEGIN { x = 3; print x }",
        ]
        .iter()
        .enumerate()
        {
            let p = dir.path().join(format!("s{i}.awk"));
            std::fs::write(&p, src).unwrap();
            let (s, ns) = file_mtime(&p).unwrap();
            cache
                .put(&p.to_string_lossy(), s, ns, &compile(src))
                .unwrap();
        }

        let (count, bytes) = cache.stats();
        assert_eq!(count, 3);
        assert!(bytes > 0, "stats must sum non-zero cp_blob bytes");
    }

    // ── CompiledProgram field roundtrip ──────────────────────────────────────
    //
    // The new parallel_safe / prog_rules_len fields on CompiledProgram are what
    // let cache hits skip AST re-parsing. If serde drops them, cache hits will
    // silently report parallel_safe=false / prog_rules_len=0 and break the
    // parallel-record fast path + range_state sizing.

    #[test]
    fn parallel_safe_round_trips_true() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = dir.path().join("t.awk");
        std::fs::write(&p, "{ print $1 }").unwrap();
        let (s, ns) = file_mtime(&p).unwrap();
        let cp = compile("{ print $1 }");
        assert!(
            cp.parallel_safe,
            "control: simple field-print is parallel-safe"
        );
        cache.put(&p.to_string_lossy(), s, ns, &cp).unwrap();
        let loaded = cache.get(&p.to_string_lossy(), s, ns).unwrap();
        assert!(
            loaded.cp.parallel_safe,
            "parallel_safe must survive cache roundtrip"
        );
        assert_eq!(loaded.cp.prog_rules_len, cp.prog_rules_len);
    }

    #[test]
    fn parallel_safe_round_trips_false() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = dir.path().join("t.awk");
        // Range patterns track state across records — definitively not parallel-safe
        // (see ast/parallel.rs::tests::parallel_unsafe_range_pattern).
        let src = "/a/,/b/ { print }";
        std::fs::write(&p, src).unwrap();
        let (s, ns) = file_mtime(&p).unwrap();
        let cp = compile(src);
        assert!(
            !cp.parallel_safe,
            "control: range patterns are not parallel-safe"
        );
        cache.put(&p.to_string_lossy(), s, ns, &cp).unwrap();
        let loaded = cache.get(&p.to_string_lossy(), s, ns).unwrap();
        assert!(
            !loaded.cp.parallel_safe,
            "parallel_safe=false must survive cache roundtrip"
        );
    }

    #[test]
    fn prog_rules_len_round_trips_multiple_rules() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("scripts.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = dir.path().join("t.awk");
        let src = "{ print $1 } { print $2 } /foo/ { print $3 }";
        std::fs::write(&p, src).unwrap();
        let (s, ns) = file_mtime(&p).unwrap();
        let cp = compile(src);
        assert_eq!(cp.prog_rules_len, 3, "control: source has 3 rules");
        cache.put(&p.to_string_lossy(), s, ns, &cp).unwrap();
        let loaded = cache.get(&p.to_string_lossy(), s, ns).unwrap();
        assert_eq!(
            loaded.cp.prog_rules_len, 3,
            "prog_rules_len must survive cache roundtrip"
        );
    }

    #[test]
    fn stats_empty_returns_zero() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("empty_cache.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let (count, bytes) = cache.stats();
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);
    }

    #[test]
    fn clear_idempotent() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("clear_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        cache.clear().unwrap();
        cache.clear().unwrap(); // second call should also succeed
    }

    #[test]
    fn evict_multiple_entries_v2() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("evict_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p1 = dir.path().join("a.awk");
        std::fs::write(&p1, "BEGIN { print 1 }").unwrap();
        let (s1, n1) = file_mtime(&p1).unwrap();
        cache
            .put("a.awk", s1, n1, &compile("BEGIN { print 1 }"))
            .unwrap();

        // Remove file p1, it should be evicted
        std::fs::remove_file(&p1).unwrap();
        let count = cache.evict_stale();
        assert_eq!(count, 1);
        let (n, _) = cache.stats();
        assert_eq!(n, 0);
    }

    #[test]
    fn stats_sums_blob_bytes_v2() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("stats_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p1 = dir.path().join("a.awk");
        std::fs::write(&p1, "BEGIN { print 1 }").unwrap();
        let (s1, n1) = file_mtime(&p1).unwrap();
        cache
            .put("a.awk", s1, n1, &compile("BEGIN { print 1 }"))
            .unwrap();

        let (count, bytes) = cache.stats();
        assert_eq!(count, 1);
        assert!(bytes > 0);
    }

    #[test]
    fn cache_overwrite_entry_v2() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("overwrite_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p1 = dir.path().join("a.awk");
        std::fs::write(&p1, "BEGIN { print 1 }").unwrap();
        let (s1, n1) = file_mtime(&p1).unwrap();
        cache
            .put("a.awk", s1, n1, &compile("BEGIN { print 1 }"))
            .unwrap();
        cache
            .put("a.awk", s1, n1, &compile("BEGIN { print 2 }"))
            .unwrap();
        let (count, _) = cache.stats();
        assert_eq!(count, 1);
    }

    #[test]
    fn cache_open_nonexistent_dir_v2() {
        let cache = ScriptCache::open(Path::new("/nonexistent/dir/cache.rkyv"));
        assert!(cache.is_err());
    }

    #[test]
    fn cache_put_get_roundtrip_v2() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("roundtrip_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = compile("BEGIN { print 1 }");
        cache.put("a.awk", 100, 200, &p).unwrap();
        let res = cache.get("a.awk", 100, 200);
        assert!(res.is_some());
    }

    #[test]
    fn cache_get_mtime_mismatch_v2() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("mismatch_test.rkyv");
        let cache = ScriptCache::open(&cache_path).unwrap();
        let p = compile("BEGIN { print 1 }");
        cache.put("a.awk", 100, 200, &p).unwrap();
        let res = cache.get("a.awk", 101, 200);
        assert!(res.is_none());
    }

    #[test]
    fn cache_enabled_logic_v3() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        let old = std::env::var("AWKRS_CACHE").ok();

        std::env::set_var("AWKRS_CACHE", "0");
        assert!(!cache_enabled());

        std::env::set_var("AWKRS_CACHE", "1");
        assert!(cache_enabled());

        if let Some(v) = old {
            std::env::set_var("AWKRS_CACHE", v);
        } else {
            std::env::remove_var("AWKRS_CACHE");
        }
    }

    #[test]
    fn read_owned_shard_corrupted_v3() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("corrupt.rkyv");
        std::fs::write(&p, b"not-a-shard").unwrap();
        assert!(read_owned_shard(&p).is_none());
    }
}
