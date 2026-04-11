# portable:2824
BEGIN {
    printf "%d\n", (78 < 46) + (46 < 9) * 2
    printf "%d\n", int(log(9 + 1) * 10)
    printf "%d\n", match("x2824yz", /[0-9]+/)
    { a1[1] = 78; a1[2] = 46; printf "%d\n", a1[1] + a1[2] }
}
