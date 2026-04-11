# portable:2920
BEGIN {
    printf "%d\n", (71 < 48) + (48 < 48) * 2
    printf "%d\n", int(log(48 + 1) * 10)
    printf "%d\n", match("x2920yz", /[0-9]+/)
    { a1[1] = 71; a1[2] = 48; printf "%d\n", a1[1] + a1[2] }
}
