# gawk parity: `printf "%u", val` of a value > 2^64 falls back to `%g`-style
# rendering (saturating at u64::MAX would lose precision).
# (The exact 2^64 case is omitted: gawk < 5.3 also falls back to %g for it,
#  gawk 5.3+ prints u64::MAX exact digits — divergent across CI gawk versions.)
BEGIN {
    printf "[%u]\n", 0
    printf "[%u]\n", 42
    printf "[%u]\n", -5         # wraps via i64→u64 two's complement
    printf "[%u]\n", 2^63       # 9223372036854775808
    printf "[%u]\n", 2^65       # %g fallback: 3.68935e+19
    printf "[%u]\n", 3e19
    printf "[%u]\n", 1e30
}
