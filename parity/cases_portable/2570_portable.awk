# portable:2570
BEGIN {
    printf "%d\n", match("x2570yz", /[0-9]+/)
    { a1[1] = 46; a1[2] = 37; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(46 + 1.0)))
    printf "%d\n", split("46:37:77", t, ":") + length(t[2])
}
