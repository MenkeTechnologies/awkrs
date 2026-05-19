# gawk parity: `printf "%d"` preserves precision out to f64's exact-integer
# range — 2^63, 2^64, etc. all print as their full decimal digits, not
# saturated at i64::MAX. (For "%u" gawk saturates at u64::MAX above 2^64.)
BEGIN {
    printf "%d|%d|%d|%d\n", 2^53, 2^63, -2^63, 1e20
    printf "%u|%u\n", 2^63, 2^64
}
