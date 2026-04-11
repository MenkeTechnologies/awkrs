# portable:2313
BEGIN {
    printf "%d\n", int(log(53 + 1) * 10)
    printf "%d\n", match("x2313yz", /[0-9]+/)
    { a1[1] = 90; a1[2] = 78; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(90 + 1.0)))
}
