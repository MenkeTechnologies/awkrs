# portable:2025
BEGIN {
    printf "%d\n", int(log(19 + 1) * 10)
    printf "%d\n", match("x2025yz", /[0-9]+/)
    { a1[1] = 14; a1[2] = 72; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(14 + 1.0)))
}
