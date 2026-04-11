# portable:2289
BEGIN {
    printf "%d\n", int(log(64 + 1) * 10)
    printf "%d\n", match("x2289yz", /[0-9]+/)
    { a1[1] = 19; a1[2] = 33; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(19 + 1.0)))
}
