# portable:2361
BEGIN {
    printf "%d\n", int(log(31 + 1) * 10)
    printf "%d\n", match("x2361yz", /[0-9]+/)
    { a1[1] = 38; a1[2] = 79; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(38 + 1.0)))
}
