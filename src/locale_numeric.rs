//! `LC_NUMERIC` setup for `-N` / `--use-lc-numeric` (Unix): C `localeconv()` decimal point.

#[cfg(unix)]
pub fn set_locale_numeric_from_env() {
    use std::ffi::CString;
    unsafe {
        let empty = CString::new("").expect("empty CString");
        libc::setlocale(libc::LC_NUMERIC, empty.as_ptr());
    }
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
