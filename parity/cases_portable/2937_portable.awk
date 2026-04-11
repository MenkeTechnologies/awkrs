# portable:2937
BEGIN {
    printf "%d\n", int(log(16 + 1) * 10)
    printf "%d\n", match("x2937yz", /[0-9]+/)
    { a1[1] = 93; a1[2] = 2; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(93 + 1.0)))
}
