# portable:2104
BEGIN {
    printf "%d\n", (82 < 31) + (31 < 7) * 2
    printf "%d\n", int(log(7 + 1) * 10)
    printf "%d\n", match("x2104yz", /[0-9]+/)
    { a1[1] = 82; a1[2] = 31; printf "%d\n", a1[1] + a1[2] }
}
