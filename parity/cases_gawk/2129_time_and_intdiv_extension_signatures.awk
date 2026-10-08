# gawk:2129 — the bundled `time` and `intdiv` extensions' signatures:
# gettimeofday() takes no argument and returns fractional epoch seconds,
# sleep() answers -1 with ERRNO for a negative argument (not a fatal), and
# intdiv(num, denom, result) fills result["quotient"] / ["remainder"] with
# the truncated quotient and the fmod remainder, returning 0.
@load "time"
@load "intdiv"
BEGIN {
    t = gettimeofday()
    print (t > 1000000000), (t < 100000000000)
    print sleep(0), sleep(-1), ERRNO
    r["stale"] = 1
    print intdiv(7, 2, r), r["quotient"], r["remainder"], ("stale" in r)
    print intdiv(-7.9, 2.5, r), r["quotient"], r["remainder"]
    print intdiv(2^53, 3, r), r["quotient"], r["remainder"]
}
