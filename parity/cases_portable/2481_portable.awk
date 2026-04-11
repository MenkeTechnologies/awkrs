# portable:2481
BEGIN {
    printf "%d\n", int(log(59 + 1) * 10)
    printf "%d\n", match("x2481yz", /[0-9]+/)
    { a1[1] = 5; a1[2] = 37; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(5 + 1.0)))
}
