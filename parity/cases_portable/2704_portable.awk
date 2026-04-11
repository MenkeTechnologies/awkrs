# portable:2704
BEGIN {
    printf "%d\n", (14 < 88) + (88 < 64) * 2
    printf "%d\n", int(log(64 + 1) * 10)
    printf "%d\n", match("x2704yz", /[0-9]+/)
    { a1[1] = 14; a1[2] = 88; printf "%d\n", a1[1] + a1[2] }
}
