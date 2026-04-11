# portable:2440
BEGIN {
    printf "%d\n", (9 < 38) + (38 < 19) * 2
    printf "%d\n", int(log(19 + 1) * 10)
    printf "%d\n", match("x2440yz", /[0-9]+/)
    { a1[1] = 9; a1[2] = 38; printf "%d\n", a1[1] + a1[2] }
}
