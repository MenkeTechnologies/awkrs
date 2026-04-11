# portable:2529
BEGIN {
    printf "%d\n", int(log(37 + 1) * 10)
    printf "%d\n", match("x2529yz", /[0-9]+/)
    { a1[1] = 50; a1[2] = 38; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(50 + 1.0)))
}
