//! `LC_NUMERIC` setup for `-N` / `--use-lc-numeric` (Unix): C `localeconv()` decimal point and
//! thousands separator for `sprintf` / `printf` / `print` / `CONVFMT` / `OFMT` and gawk **`%'`**.
//!
//! **Not affected:** Coercing field strings and other input text to numbers (e.g. `$1` compared as
//! number, `strtonum`) still treats **`.`** as the decimal radix—locale-aware numeric **input** is not
//! implemented (same as README **`-N`** / **Locale & pipes**).

#[cfg(unix)]
pub fn set_locale_numeric_from_env() {
    // setlocale is process-global mutable state; calling it from multiple
    // threads concurrently is UB and produces SIGSEGV/SIGBUS in libc. Runtime::new()
    // calls this on every construction, so under parallel tests we hit the race.
    // Once::call_once gives at-most-once semantics with internal synchronization —
    // the first caller activates LC_NUMERIC, the rest become no-ops.
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        use std::ffi::CString;
        unsafe {
            let empty = CString::new("").expect("empty CString");
            libc::setlocale(libc::LC_NUMERIC, empty.as_ptr());
        }
    });
}

/// Thousands separator from `localeconv()` (gawk **`%'`** integer grouping). Empty means
/// "no separator" in the C/POSIX locale — gawk's `%'` flag is then skipped (no grouping).
/// Callers that want unconditional `,` grouping should set `LC_NUMERIC` to a locale that
/// provides one (e.g. `en_US.UTF-8`). Apple's libc reports `,` even in C; glibc reports
/// empty in C — using the locale's actual value matches gawk's documented behavior.
#[cfg(unix)]
pub fn thousands_sep_from_locale() -> Option<char> {
    use std::ffi::CStr;
    unsafe {
        let lc = libc::localeconv();
        if lc.is_null() {
            return Some(',');
        }
        let p = (*lc).thousands_sep;
        if p.is_null() {
            return Some(',');
        }
        let s = CStr::from_ptr(p);
        let b = s.to_bytes();
        if b.is_empty() {
            None
        } else {
            std::str::from_utf8(b).ok().and_then(|t| t.chars().next())
        }
    }
}

#[cfg(not(unix))]
pub fn thousands_sep_from_locale() -> Option<char> {
    Some(',')
}

#[cfg(unix)]
pub fn decimal_point_from_locale() -> char {
    use std::ffi::CStr;
    unsafe {
        let lc = libc::localeconv();
        if lc.is_null() {
            return '.';
        }
        let dp = (*lc).decimal_point;
        if dp.is_null() {
            return '.';
        }
        let s = CStr::from_ptr(dp);
        let b = s.to_bytes();
        if b.is_empty() {
            return '.';
        }
        std::str::from_utf8(b)
            .ok()
            .and_then(|t| t.chars().next())
            .unwrap_or('.')
    }
}

#[cfg(not(unix))]
pub fn set_locale_numeric_from_env() {}

#[cfg(not(unix))]
pub fn decimal_point_from_locale() -> char {
    '.'
}

#[cfg(test)]
mod tests {
    #[cfg(not(unix))]
    #[test]
    fn decimal_point_is_ascii_dot_on_non_unix() {
        assert_eq!(super::decimal_point_from_locale(), '.');
    }

    #[cfg(not(unix))]
    #[test]
    fn thousands_sep_comma_on_non_unix() {
        assert_eq!(super::thousands_sep_from_locale(), Some(','));
    }

    #[test]
    fn set_locale_numeric_from_env_does_not_panic() {
        super::set_locale_numeric_from_env();
    }

    #[cfg(unix)]
    #[test]
    fn decimal_point_is_valid_char() {
        let dp = super::decimal_point_from_locale();
        assert!(dp == '.' || dp == ',');
    }

    #[cfg(unix)]
    #[test]
    fn thousands_sep_is_valid_or_none() {
        let ts = super::thousands_sep_from_locale();
        if let Some(c) = ts {
            assert!(c == ',' || c == '.' || c == ' ' || c == '\u{a0}' || c == '\u{202f}');
        }
    }
}
