# portable:2265
BEGIN {
    printf "%d\n", int(log(75 + 1) * 10)
    printf "%d\n", match("x2265yz", /[0-9]+/)
    { a1[1] = 45; a1[2] = 77; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(45 + 1.0)))
}
