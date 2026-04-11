# portable:2848
BEGIN {
    printf "%d\n", (52 < 2) + (2 < 81) * 2
    printf "%d\n", int(log(81 + 1) * 10)
    printf "%d\n", match("x2848yz", /[0-9]+/)
    { a1[1] = 52; a1[2] = 2; printf "%d\n", a1[1] + a1[2] }
}
