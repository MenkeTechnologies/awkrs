# portable:2865
BEGIN {
    printf "%d\n", int(log(49 + 1) * 10)
    printf "%d\n", match("x2865yz", /[0-9]+/)
    { a1[1] = 74; a1[2] = 45; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(74 + 1.0)))
}
