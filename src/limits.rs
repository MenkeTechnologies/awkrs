//! Hard limits to avoid unbounded host stack use and pathological recursion.

/// Maximum nested depth for user-defined function calls (VM and interpreter).
///
/// Kept conservative: each depth level uses native stack in the VM/interpreter, so a very large
/// limit can still overflow the host stack before the check trips.
#[cfg(test)]
pub const MAX_USER_CALL_DEPTH: usize = 32;
#[cfg(not(test))]
pub const MAX_USER_CALL_DEPTH: usize = 256;
