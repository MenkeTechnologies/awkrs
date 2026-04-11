# portable:2553
BEGIN {
    printf "%d\n", int(log(26 + 1) * 10)
    printf "%d\n", match("x2553yz", /[0-9]+/)
    { a1[1] = 24; a1[2] = 83; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(24 + 1.0)))
}
