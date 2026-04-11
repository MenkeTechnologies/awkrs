# portable:2080
BEGIN {
    printf "%d\n", (11 < 75) + (75 < 18) * 2
    printf "%d\n", int(log(18 + 1) * 10)
    printf "%d\n", match("x2080yz", /[0-9]+/)
    { a1[1] = 11; a1[2] = 75; printf "%d\n", a1[1] + a1[2] }
}
