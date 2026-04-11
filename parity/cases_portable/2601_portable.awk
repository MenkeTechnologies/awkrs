# portable:2601
BEGIN {
    printf "%d\n", int(log(4 + 1) * 10)
    printf "%d\n", match("x2601yz", /[0-9]+/)
    { a1[1] = 69; a1[2] = 84; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(69 + 1.0)))
}
