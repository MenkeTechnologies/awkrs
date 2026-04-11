# portable:2457
BEGIN {
    printf "%d\n", int(log(70 + 1) * 10)
    printf "%d\n", match("x2457yz", /[0-9]+/)
    { a1[1] = 31; a1[2] = 81; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(31 + 1.0)))
}
