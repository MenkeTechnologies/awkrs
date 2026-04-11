# portable:2944
BEGIN {
    printf "%d\n", (45 < 4) + (4 < 37) * 2
    printf "%d\n", int(log(37 + 1) * 10)
    printf "%d\n", match("x2944yz", /[0-9]+/)
    { a1[1] = 45; a1[2] = 4; printf "%d\n", a1[1] + a1[2] }
}
