# portable:2001
BEGIN {
    printf "%d\n", int(log(30 + 1) * 10)
    printf "%d\n", match("x2001yz", /[0-9]+/)
    { a1[1] = 40; a1[2] = 27; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(40 + 1.0)))
}
