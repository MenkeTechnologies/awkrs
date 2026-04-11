# portable:2769
BEGIN {
    printf "%d\n", int(log(10 + 1) * 10)
    printf "%d\n", match("x2769yz", /[0-9]+/)
    { a1[1] = 81; a1[2] = 43; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(81 + 1.0)))
}
