# portable:2697
BEGIN {
    printf "%d\n", int(log(43 + 1) * 10)
    printf "%d\n", match("x2697yz", /[0-9]+/)
    { a1[1] = 62; a1[2] = 86; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(62 + 1.0)))
}
