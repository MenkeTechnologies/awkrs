# portable:2625
BEGIN {
    printf "%d\n", int(log(76 + 1) * 10)
    printf "%d\n", match("x2625yz", /[0-9]+/)
    { a1[1] = 43; a1[2] = 40; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(43 + 1.0)))
}
