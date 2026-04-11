# portable:2913
BEGIN {
    printf "%d\n", int(log(27 + 1) * 10)
    printf "%d\n", match("x2913yz", /[0-9]+/)
    { a1[1] = 22; a1[2] = 46; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(22 + 1.0)))
}
