# portable:2968
BEGIN {
    printf "%d\n", (19 < 49) + (49 < 26) * 2
    printf "%d\n", int(log(26 + 1) * 10)
    printf "%d\n", match("x2968yz", /[0-9]+/)
    { a1[1] = 19; a1[2] = 49; printf "%d\n", a1[1] + a1[2] }
}
