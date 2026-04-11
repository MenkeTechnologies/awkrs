# portable:2056
BEGIN {
    printf "%d\n", (37 < 30) + (30 < 29) * 2
    printf "%d\n", int(log(29 + 1) * 10)
    printf "%d\n", match("x2056yz", /[0-9]+/)
    { a1[1] = 37; a1[2] = 30; printf "%d\n", a1[1] + a1[2] }
}
