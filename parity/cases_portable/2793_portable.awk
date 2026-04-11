# portable:2793
BEGIN {
    printf "%d\n", int(log(82 + 1) * 10)
    printf "%d\n", match("x2793yz", /[0-9]+/)
    { a1[1] = 55; a1[2] = 88; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(55 + 1.0)))
}
