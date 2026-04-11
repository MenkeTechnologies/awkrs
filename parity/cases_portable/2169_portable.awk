# portable:2169
BEGIN {
    printf "%d\n", int(log(36 + 1) * 10)
    printf "%d\n", match("x2169yz", /[0-9]+/)
    { a1[1] = 52; a1[2] = 75; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(52 + 1.0)))
}
