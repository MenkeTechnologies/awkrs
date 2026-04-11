# portable:2296
BEGIN {
    printf "%d\n", (68 < 35) + (35 < 85) * 2
    printf "%d\n", int(log(85 + 1) * 10)
    printf "%d\n", match("x2296yz", /[0-9]+/)
    { a1[1] = 68; a1[2] = 35; printf "%d\n", a1[1] + a1[2] }
}
