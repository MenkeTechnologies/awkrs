# portable:2073
BEGIN {
    printf "%d\n", int(log(80 + 1) * 10)
    printf "%d\n", match("x2073yz", /[0-9]+/)
    { a1[1] = 59; a1[2] = 73; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(59 + 1.0)))
}
