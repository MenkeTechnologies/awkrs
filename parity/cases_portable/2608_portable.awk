# portable:2608
BEGIN {
    printf "%d\n", (21 < 86) + (86 < 25) * 2
    printf "%d\n", int(log(25 + 1) * 10)
    printf "%d\n", match("x2608yz", /[0-9]+/)
    { a1[1] = 21; a1[2] = 86; printf "%d\n", a1[1] + a1[2] }
}
