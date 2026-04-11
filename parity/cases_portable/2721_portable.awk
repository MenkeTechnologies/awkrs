# portable:2721
BEGIN {
    printf "%d\n", int(log(32 + 1) * 10)
    printf "%d\n", match("x2721yz", /[0-9]+/)
    { a1[1] = 36; a1[2] = 42; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(36 + 1.0)))
}
