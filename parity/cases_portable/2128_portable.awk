# portable:2128
BEGIN {
    printf "%d\n", (56 < 76) + (76 < 79) * 2
    printf "%d\n", int(log(79 + 1) * 10)
    printf "%d\n", match("x2128yz", /[0-9]+/)
    { a1[1] = 56; a1[2] = 76; printf "%d\n", a1[1] + a1[2] }
}
