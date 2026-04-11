# portable:2745
BEGIN {
    printf "%d\n", int(log(21 + 1) * 10)
    printf "%d\n", match("x2745yz", /[0-9]+/)
    { a1[1] = 10; a1[2] = 87; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(10 + 1.0)))
}
