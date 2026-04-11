# portable:2985
BEGIN {
    printf "%d\n", int(log(77 + 1) * 10)
    printf "%d\n", match("x2985yz", /[0-9]+/)
    { a1[1] = 41; a1[2] = 3; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(41 + 1.0)))
}
