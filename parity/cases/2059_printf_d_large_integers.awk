# gawk parity: `printf "%d"` preserves precision out to f64's exact-integer
# range — 2^53, 2^63, 1e20 all print as their full decimal digits.
# (The 2^64 `%u` boundary case is omitted: gawk < 5.3 falls back to %g
#  rendering, gawk 5.3+ prints exact u64::MAX digits — divergent.)
BEGIN {
    printf "%d|%d|%d|%d\n", 2^53, 2^63, -2^63, 1e20
    printf "%u\n", 2^63
}
