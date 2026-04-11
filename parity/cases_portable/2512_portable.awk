# portable:2512
BEGIN {
    printf "%d\n", (28 < 84) + (84 < 69) * 2
    printf "%d\n", int(log(69 + 1) * 10)
    printf "%d\n", match("x2512yz", /[0-9]+/)
    { a1[1] = 28; a1[2] = 84; printf "%d\n", a1[1] + a1[2] }
}
