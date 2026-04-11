# portable:2649
BEGIN {
    printf "%d\n", int(log(65 + 1) * 10)
    printf "%d\n", match("x2649yz", /[0-9]+/)
    { a1[1] = 17; a1[2] = 85; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(17 + 1.0)))
}
