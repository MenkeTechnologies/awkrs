//! `LC_NUMERIC` setup for `-N` / `--use-lc-numeric` (Unix): C `localeconv()` decimal point and
//! thousands separator for sprintf/printf/`%'` — string→number parsing of user data still uses **`.`**
//! (see README **Not “all awks”** / **`-N`**).

#[cfg(unix)]
pub fn set_locale_numeric_from_env() {
    use std::ffi::CString;
    unsafe {
        let empty = CString::new("").expect("empty CString");
        libc::setlocale(libc::LC_NUMERIC, empty.as_ptr());
    }
}

/// Thousands separator from `localeconv()` (gawk **`%'`** integer grouping). Empty means “no separator”
/// in the C locale; callers may fall back to **`,`** for **`%'`** formatting.
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
}
