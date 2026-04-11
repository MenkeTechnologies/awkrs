# portable:2008
BEGIN {
    printf "%d\n", (89 < 29) + (29 < 51) * 2
    printf "%d\n", int(log(51 + 1) * 10)
    printf "%d\n", match("x2008yz", /[0-9]+/)
    { a1[1] = 89; a1[2] = 29; printf "%d\n", a1[1] + a1[2] }
}
