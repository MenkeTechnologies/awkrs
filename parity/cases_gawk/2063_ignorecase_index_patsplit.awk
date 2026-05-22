# gawk parity: `IGNORECASE` applies to `index()` and `patsplit()` (in addition
# to the already-honored split/match/sub/gsub). Previously awkrs ignored it
# for these two.
BEGIN {
    IGNORECASE = 1

    print "index:"
    print index("ABCabc", "b")
    print index("Hello World", "WORLD")
    print index("xyz", "A")

    print "patsplit:"
    n = patsplit("xBYbZBA", a, "b")
    print n
    for (i = 1; i <= n; i++) print i, a[i]
}
