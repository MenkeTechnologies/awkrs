# portable:2097
BEGIN {
    printf "%d\n", int(log(69 + 1) * 10)
    printf "%d\n", match("x2097yz", /[0-9]+/)
    { a1[1] = 33; a1[2] = 29; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(33 + 1.0)))
}
