# gawk:2124 — gawk's integer conversions decide "the value is zero" from the
# value (printf.c is_zero), not from its truncated digits: `%.0d` of 0.5 still
# prints `0`, and `#` still adds `0x` to a hex 0.5. Exact zero keeps the POSIX
# empty `%.0d` and the bare `0` for `%#x`.
BEGIN {
    split("0.5 -0.5 0.9 1e-300 0 -0", v, " ")
    for (i = 1; i <= 6; i++) {
        x = v[i] + 0
        printf "%s: [%.0d] [%.0i] [%.0u] [%.0o] [%.0x] [%#x] [%#.0X] [%#5x] [%#-6x] [%+.0d] [% .0d] [%5.0d]\n",
            v[i], x, x, x, x, x, x, x, x, x, x, x, x
    }
}
