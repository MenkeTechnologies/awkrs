# portable:2409
BEGIN {
    printf "%d\n", int(log(9 + 1) * 10)
    printf "%d\n", match("x2409yz", /[0-9]+/)
    { a1[1] = 83; a1[2] = 80; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(83 + 1.0)))
}
