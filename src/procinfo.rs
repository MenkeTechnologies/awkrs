//! gawk-style [`crate::runtime::Runtime::PROCINFO`] keys (best-effort).

use crate::bytecode::CompiledProgram;
use crate::runtime::{AwkMap, Runtime, Value};
use std::ffi::CStr;

/// gawk uses **`posix`** / **`mingw`** / **`vms`**, not Rust’s `std::env::consts::OS` (`macos`, `linux`, …).
///
/// `target_os = "vms"` is reserved for a future OpenVMS Rust target (currently unused on all tier-1
/// platforms).
pub(crate) fn gawk_platform_string() -> &'static str {
    #[cfg(target_os = "vms")]
    {
        return "vms";
    }
    if cfg!(unix) {
        "posix"
    } else if cfg!(windows) {
        "mingw"
    } else {
        "unknown"
    }
}

/// When `Some`, awkrs was built with gawk-style PMA and **`PROCINFO["pma"]`** is set (see gawk manual).
/// Default `None` matches gawk built **without** PMA (key omitted).
pub const AWKRS_PMA_VERSION: Option<&'static str> = None;

/// gawk: if **`PROCINFO["READ_TIMEOUT"]`** is absent, initialize from **`GAWK_READ_TIMEOUT`** (milliseconds).
pub(crate) fn gawk_read_timeout_env() -> i32 {
    std::env::var("GAWK_READ_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(|n| n.clamp(0, i32::MAX as i64) as i32)
        .unwrap_or(0)
}

/// Reflects active field-splitting mode (gawk **`PROCINFO["FS"]`**).
pub(crate) fn field_split_mode(rt: &Runtime) -> &'static str {
    if rt.csv_mode {
        return "API";
    }
    let fw = rt
        .get_global_var("FIELDWIDTHS")
        .map(|v| v.as_str().to_string())
        .unwrap_or_default();
    if !fw.trim().is_empty() {
        return "FIELDWIDTHS";
    }
    let fp = rt
        .get_global_var("FPAT")
        .map(|v| v.as_str().to_string())
        .unwrap_or_default();
    if !fp.trim().is_empty() {
        return "FPAT";
    }
    "FS"
}

/// Nested **`PROCINFO["identifiers"]`**: name → type string (gawk-style).
pub(crate) fn merge_procinfo_identifiers(p: &mut AwkMap<String, Value>, cp: &CompiledProgram) {
    let mut id = AwkMap::default();
    for &name in crate::namespace::BUILTIN_NAMES {
        id.insert(name.into(), Value::Str("builtin".into()));
    }
    for name in &cp.slot_names {
        if name.is_empty() {
            continue;
        }
        id.insert(name.clone(), Value::Str("scalar".into()));
    }
    for name in &cp.array_var_names {
        id.insert(name.clone(), Value::Str("array".into()));
    }
    for name in cp.functions.keys() {
        id.insert(name.clone(), Value::Str("user".into()));
    }
    p.insert("identifiers".into(), Value::Array(id));
}

pub(crate) fn gmp_version_string() -> String {
    unsafe { c_str_to_string(gmp_mpfr_sys::gmp::version) }
}

pub(crate) fn mpfr_version_string() -> String {
    unsafe { c_str_to_string(gmp_mpfr_sys::mpfr::get_version()) }
}

unsafe fn c_str_to_string(p: *const libc::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

#[cfg(unix)]
pub(crate) fn supplementary_group_entries() -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let ng = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if ng <= 0 {
        return out;
    }
    let mut buf = vec![0 as libc::gid_t; ng as usize];
    let n = unsafe { libc::getgroups(ng, buf.as_mut_ptr()) };
    if n <= 0 {
        return out;
    }
    for (i, &g) in buf.iter().take(n as usize).enumerate() {
        out.push((format!("group{}", i + 1), g as f64));
    }
    out
}

#[cfg(not(unix))]
pub(crate) fn supplementary_group_entries() -> Vec<(String, f64)> {
    Vec::new()
}

/// Locale-dependent max bytes per multibyte character (best-effort).
#[cfg(all(test, unix))]
#[test]
fn gawk_platform_unix_reports_posix_not_rust_os() {
    assert_eq!(
        gawk_platform_string(),
        "posix",
        "PROCINFO[\"platform\"] must follow gawk, not std::env::consts::OS"
    );
}

pub(crate) fn mb_cur_max_value() -> f64 {
    #[cfg(target_os = "linux")]
    {
        let v = unsafe { libc::sysconf(libc::_SC_MB_LEN_MAX) };
        if v > 0 {
            return v as f64;
        }
    }
    if cfg!(unix) {
        6.0
    } else {
        1.0
    }
}

#[cfg(all(test, windows))]
#[test]
fn gawk_platform_windows_reports_mingw() {
    assert_eq!(gawk_platform_string(), "mingw");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Runtime, Value};

    #[test]
    fn field_split_mode_fs_by_default() {
        let rt = Runtime::new();
        assert_eq!(field_split_mode(&rt), "FS");
    }

    #[test]
    fn field_split_mode_csv_reports_api() {
        let mut rt = Runtime::new();
        rt.csv_mode = true;
        assert_eq!(field_split_mode(&rt), "API");
    }

    #[test]
    fn field_split_mode_fieldwidths_over_fpat() {
        let mut rt = Runtime::new();
        rt.vars
            .insert("FIELDWIDTHS".into(), Value::Str("1 2".into()));
        rt.vars.insert("FPAT".into(), Value::Str("[0-9]+".into()));
        assert_eq!(field_split_mode(&rt), "FIELDWIDTHS");
    }

    #[test]
    fn field_split_mode_fpat_when_no_fieldwidths() {
        let mut rt = Runtime::new();
        rt.vars.insert("FPAT".into(), Value::Str("[0-9]+".into()));
        assert_eq!(field_split_mode(&rt), "FPAT");
    }

    #[test]
    fn field_split_mode_whitespace_only_fieldwidths_ignored() {
        let mut rt = Runtime::new();
        rt.vars
            .insert("FIELDWIDTHS".into(), Value::Str("  \t".into()));
        rt.vars.insert("FPAT".into(), Value::Str("[0-9]+".into()));
        assert_eq!(field_split_mode(&rt), "FPAT");
    }

    #[test]
    fn field_split_mode_non_whitespace_fieldwidths_wins_over_fpat() {
        let mut rt = Runtime::new();
        rt.vars.insert("FIELDWIDTHS".into(), Value::Str("1".into()));
        rt.vars.insert("FPAT".into(), Value::Str("[0-9]+".into()));
        assert_eq!(field_split_mode(&rt), "FIELDWIDTHS");
    }

    #[test]
    fn field_split_mode_empty_fieldwidths_string_uses_fpat() {
        let mut rt = Runtime::new();
        rt.vars.insert("FIELDWIDTHS".into(), Value::Str("".into()));
        rt.vars.insert("FPAT".into(), Value::Str("[a-z]+".into()));
        assert_eq!(field_split_mode(&rt), "FPAT");
    }

    #[test]
    fn gawk_read_timeout_env_missing_zero() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::remove_var("GAWK_READ_TIMEOUT");
        assert_eq!(gawk_read_timeout_env(), 0);
    }

    #[test]
    fn gawk_read_timeout_env_parses_ms() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::set_var("GAWK_READ_TIMEOUT", "123");
        assert_eq!(gawk_read_timeout_env(), 123);
        std::env::remove_var("GAWK_READ_TIMEOUT");
    }

    #[test]
    fn gawk_read_timeout_env_invalid_falls_back_zero() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::set_var("GAWK_READ_TIMEOUT", "not_a_number");
        assert_eq!(gawk_read_timeout_env(), 0);
        std::env::remove_var("GAWK_READ_TIMEOUT");
    }

    #[test]
    fn gawk_read_timeout_env_empty_string_zero() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::set_var("GAWK_READ_TIMEOUT", "");
        assert_eq!(gawk_read_timeout_env(), 0);
        std::env::remove_var("GAWK_READ_TIMEOUT");
    }

    #[test]
    fn gawk_read_timeout_env_negative_clamps_zero() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::set_var("GAWK_READ_TIMEOUT", "-5");
        assert_eq!(gawk_read_timeout_env(), 0);
        std::env::remove_var("GAWK_READ_TIMEOUT");
    }

    #[test]
    fn gawk_read_timeout_env_huge_clamps_i32_max() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        std::env::set_var("GAWK_READ_TIMEOUT", "999999999999");
        assert_eq!(gawk_read_timeout_env(), i32::MAX);
        std::env::remove_var("GAWK_READ_TIMEOUT");
    }

    #[test]
    fn mb_cur_max_positive() {
        let v = mb_cur_max_value();
        assert!(v >= 1.0, "mb_cur_max={v}");
    }

    #[test]
    fn awkrs_pma_version_omitted_by_default() {
        assert!(
            AWKRS_PMA_VERSION.is_none(),
            "built without gawk PMA — key should stay unset unless explicitly wired"
        );
    }

    #[test]
    fn merge_procinfo_identifiers_tags_builtins_slots_arrays_and_functions() {
        use crate::bytecode::{Chunk, CompiledFunc, CompiledProgram, StringPool};
        use std::collections::HashMap;

        let cp = CompiledProgram {
            begin_chunks: vec![],
            end_chunks: vec![],
            beginfile_chunks: vec![],
            endfile_chunks: vec![],
            record_rules: vec![],
            functions: HashMap::from([(
                "uf".into(),
                CompiledFunc {
                    params: vec![],
                    body: Chunk::from_ops(vec![]),
                },
            )]),
            strings: StringPool::default(),
            slot_count: 1,
            slot_names: vec!["vx".into()],
            slot_map: HashMap::from([("vx".into(), 0u16)]),
            array_var_names: vec!["arr".into()],
        };
        let mut proc = AwkMap::default();
        merge_procinfo_identifiers(&mut proc, &cp);
        let id = match proc.get("identifiers").expect("identifiers key") {
            Value::Array(m) => m,
            v => panic!("expected PROCINFO identifiers map, got {v:?}"),
        };
        assert_eq!(id.get("split").unwrap().as_str(), "builtin");
        assert_eq!(id.get("vx").unwrap().as_str(), "scalar");
        assert_eq!(id.get("arr").unwrap().as_str(), "array");
        assert_eq!(id.get("uf").unwrap().as_str(), "user");
    }
}
