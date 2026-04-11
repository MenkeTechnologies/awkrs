# portable:2145
BEGIN {
    printf "%d\n", int(log(47 + 1) * 10)
    printf "%d\n", match("x2145yz", /[0-9]+/)
    { a1[1] = 78; a1[2] = 30; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(78 + 1.0)))
}
