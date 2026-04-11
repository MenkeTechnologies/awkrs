# portable:2841
BEGIN {
    printf "%d\n", int(log(60 + 1) * 10)
    printf "%d\n", match("x2841yz", /[0-9]+/)
    { a1[1] = 3; a1[2] = 89; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(3 + 1.0)))
}
