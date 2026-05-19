# gawk parity: a regex that can match the empty string contributes no splits
# at positions where it matched zero-width. `split("abc", a, /x*/)` returns
# 1 field "abc" — NOT one split per byte position.
BEGIN {
    print "===  /x*/ on abc ==="
    n = split("abc", a, /x*/)
    print n, "[" a[1] "]"

    print "===  /a*/ on aaab ==="
    n = split("aaab", b, /a*/)
    print n
    for (i = 1; i <= n; i++) printf "%d=[%s]\n", i, b[i]

    print "===  /[0-9]+/ on a1b22c333 ==="
    n = split("a1b22c333", c, /[0-9]+/)
    print n
    for (i = 1; i <= n; i++) printf "%d=[%s]\n", i, c[i]
}
