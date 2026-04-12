BEGIN {
    n = patsplit("foo:bar::baz", a, /[a-z]+/)
    print n
    for (i = 1; i <= n; i++) printf "%s ", a[i]
    print ""

    n = patsplit("12ab34cd56", b, /[0-9]+/)
    print n
    for (i = 1; i <= n; i++) printf "%s ", b[i]
    print ""

    n = patsplit("no-match-here", c, /[0-9]+/)
    print n
}
