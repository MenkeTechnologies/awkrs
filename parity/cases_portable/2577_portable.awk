# portable:2577
BEGIN {
    printf "%d\n", int(log(15 + 1) * 10)
    printf "%d\n", match("x2577yz", /[0-9]+/)
    { a1[1] = 95; a1[2] = 39; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(95 + 1.0)))
}
