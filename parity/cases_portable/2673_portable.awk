# portable:2673
BEGIN {
    printf "%d\n", int(log(54 + 1) * 10)
    printf "%d\n", match("x2673yz", /[0-9]+/)
    { a1[1] = 88; a1[2] = 41; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(88 + 1.0)))
}
