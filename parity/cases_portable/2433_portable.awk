# portable:2433
BEGIN {
    printf "%d\n", int(log(81 + 1) * 10)
    printf "%d\n", match("x2433yz", /[0-9]+/)
    { a1[1] = 57; a1[2] = 36; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(57 + 1.0)))
}
