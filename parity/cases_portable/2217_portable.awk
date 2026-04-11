# portable:2217
BEGIN {
    printf "%d\n", int(log(14 + 1) * 10)
    printf "%d\n", match("x2217yz", /[0-9]+/)
    { a1[1] = 97; a1[2] = 76; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(97 + 1.0)))
}
