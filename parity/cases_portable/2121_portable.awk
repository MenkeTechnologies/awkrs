# portable:2121
BEGIN {
    printf "%d\n", int(log(58 + 1) * 10)
    printf "%d\n", match("x2121yz", /[0-9]+/)
    { a1[1] = 7; a1[2] = 74; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(7 + 1.0)))
}
