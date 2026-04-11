# portable:2889
BEGIN {
    printf "%d\n", int(log(38 + 1) * 10)
    printf "%d\n", match("x2889yz", /[0-9]+/)
    { a1[1] = 48; a1[2] = 90; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(48 + 1.0)))
}
