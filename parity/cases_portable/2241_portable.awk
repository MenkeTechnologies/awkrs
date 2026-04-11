# portable:2241
BEGIN {
    printf "%d\n", int(log(3 + 1) * 10)
    printf "%d\n", match("x2241yz", /[0-9]+/)
    { a1[1] = 71; a1[2] = 32; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(71 + 1.0)))
}
