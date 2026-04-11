# portable:2337
BEGIN {
    printf "%d\n", int(log(42 + 1) * 10)
    printf "%d\n", match("x2337yz", /[0-9]+/)
    { a1[1] = 64; a1[2] = 34; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(64 + 1.0)))
}
