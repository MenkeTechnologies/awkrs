# portable:2817
BEGIN {
    printf "%d\n", int(log(71 + 1) * 10)
    printf "%d\n", match("x2817yz", /[0-9]+/)
    { a1[1] = 29; a1[2] = 44; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(29 + 1.0)))
}
