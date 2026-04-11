//! Hard limits to avoid unbounded host stack use and pathological recursion.

/// Maximum nested depth for user-defined function calls (VM).
///
/// Kept conservative: each depth level uses native stack in the VM, so a very large
/// limit can still overflow the host stack before the check trips.
#[cfg(test)]
pub const MAX_USER_CALL_DEPTH: usize = 32;
#[cfg(not(test))]
pub const MAX_USER_CALL_DEPTH: usize = 256;

#[cfg(test)]
mod tests {
    #[test]
    fn test_build_uses_lower_call_depth_cap() {
        assert_eq!(super::MAX_USER_CALL_DEPTH, 32);
    }
}
