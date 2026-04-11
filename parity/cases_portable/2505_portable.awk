# portable:2505
BEGIN {
    printf "%d\n", int(log(48 + 1) * 10)
    printf "%d\n", match("x2505yz", /[0-9]+/)
    { a1[1] = 76; a1[2] = 82; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(76 + 1.0)))
}
