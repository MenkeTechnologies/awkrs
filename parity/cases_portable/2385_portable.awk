# portable:2385
BEGIN {
    printf "%d\n", int(log(20 + 1) * 10)
    printf "%d\n", match("x2385yz", /[0-9]+/)
    { a1[1] = 12; a1[2] = 35; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(12 + 1.0)))
}
