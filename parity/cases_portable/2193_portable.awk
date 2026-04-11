# portable:2193
BEGIN {
    printf "%d\n", int(log(25 + 1) * 10)
    printf "%d\n", match("x2193yz", /[0-9]+/)
    { a1[1] = 26; a1[2] = 31; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(26 + 1.0)))
}
