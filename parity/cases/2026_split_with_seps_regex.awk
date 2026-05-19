BEGIN {
    n = split("a1b22c333d", a, /[0-9]+/, seps)
    print "n=" n
    for (i = 1; i <= n; i++) printf "a[%d]=%s\n", i, a[i]
    for (i = 1; i < n; i++) printf "seps[%d]=%s\n", i, seps[i]
}
