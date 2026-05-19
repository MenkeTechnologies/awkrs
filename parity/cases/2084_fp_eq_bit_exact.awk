# gawk parity: numeric `==` is bit-exact (POSIX). Earlier awkrs used a fuzzy
# `f64::EPSILON` tolerance, so `0.1 + 0.2 == 0.3` reported true (the
# difference is ~5.55e-17, below EPSILON). gawk returns 0.
BEGIN {
    print (0.1 + 0.2 == 0.3)
    print (1.0 == 1), (1e0 == 1)
    print (1.5 == 1.5), (-0.0 == 0.0)
    # Very near but not equal: still 0.
    print (0.0 == 1e-300)
    # Two derivations that differ only by a single ulp.
    x = 1.0 / 3.0
    y = 0.3333333333333333
    print (x == y)
}
