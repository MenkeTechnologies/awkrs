BEGIN {
    n = split("  alpha   beta\tgamma\t\tdelta  ", a, " ", seps)
    print "n=" n
    for (i = 1; i <= n; i++) printf "a[%d]=[%s]\n", i, a[i]
    for (i = 1; i < n; i++) printf "seps[%d]=<%s>\n", i, seps[i]
}
