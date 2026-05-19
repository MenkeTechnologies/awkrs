# gawk parity: `printf "%u", val` of a value > 2^64 falls back to `%g`-style
# rendering (saturating at u64::MAX would lose precision). The exact 2^64
# boundary still prints as the u64::MAX digit string.
BEGIN {
    printf "[%u]\n", 0
    printf "[%u]\n", 42
    printf "[%u]\n", -5         # wraps via i64→u64 two's complement
    printf "[%u]\n", 2^63       # 9223372036854775808
    printf "[%u]\n", 2^64       # u64::MAX
    printf "[%u]\n", 2^65       # %g fallback: 3.68935e+19
    printf "[%u]\n", 3e19
    printf "[%u]\n", 1e30
}
