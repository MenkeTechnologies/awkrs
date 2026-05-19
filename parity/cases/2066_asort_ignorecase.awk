# gawk parity: `asort` and `asorti` honor `IGNORECASE` for string comparison.
BEGIN {
    IGNORECASE = 1

    print "asort:"
    a[1] = "B"; a[2] = "a"; a[3] = "C"; a[4] = "d"
    n = asort(a)
    for (i = 1; i <= n; i++) print a[i]

    print "asorti:"
    delete b
    b["B"] = 1; b["a"] = 2; b["C"] = 3; b["d"] = 4
    n = asorti(b)
    for (i = 1; i <= n; i++) print b[i]
}
