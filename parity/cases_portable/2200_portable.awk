# portable:2200
BEGIN {
    printf "%d\n", (75 < 33) + (33 < 46) * 2
    printf "%d\n", int(log(46 + 1) * 10)
    printf "%d\n", match("x2200yz", /[0-9]+/)
    { a1[1] = 75; a1[2] = 33; printf "%d\n", a1[1] + a1[2] }
}
