//! Hard limits to avoid unbounded host stack use and pathological recursion.

/// Maximum nested depth for user-defined function calls (VM).
///
/// Kept conservative: each depth level uses native stack in the VM, so a very large
/// limit can still overflow the host stack before the check trips. Empirically
/// the host thread (default 8 MiB Rust stack) starts overflowing around 200
/// frames in unoptimized builds, so the production cap stays comfortably below
/// that.
#[cfg(test)]
pub const MAX_USER_CALL_DEPTH: usize = 32;
#[cfg(not(test))]
pub const MAX_USER_CALL_DEPTH: usize = 150;

#[cfg(test)]
mod tests {
    #[test]
    fn test_build_uses_lower_call_depth_cap() {
        assert_eq!(super::MAX_USER_CALL_DEPTH, 32);
    }

    #[test]
    fn max_user_call_depth_is_positive_v7() {
        assert!(super::MAX_USER_CALL_DEPTH > 0);
    }

    #[test]
    fn limits_cap_is_small_v29() {
        assert!(super::MAX_USER_CALL_DEPTH < 1000);
    }
}
