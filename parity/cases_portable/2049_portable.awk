# portable:2049
BEGIN {
    printf "%d\n", int(log(8 + 1) * 10)
    printf "%d\n", match("x2049yz", /[0-9]+/)
    { a1[1] = 85; a1[2] = 28; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(85 + 1.0)))
}
