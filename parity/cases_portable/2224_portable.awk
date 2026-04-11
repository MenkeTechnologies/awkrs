# portable:2224
BEGIN {
    printf "%d\n", (49 < 78) + (78 < 35) * 2
    printf "%d\n", int(log(35 + 1) * 10)
    printf "%d\n", match("x2224yz", /[0-9]+/)
    { a1[1] = 49; a1[2] = 78; printf "%d\n", a1[1] + a1[2] }
}
