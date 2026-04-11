# portable:2392
BEGIN {
    printf "%d\n", (61 < 37) + (37 < 41) * 2
    printf "%d\n", int(log(41 + 1) * 10)
    printf "%d\n", match("x2392yz", /[0-9]+/)
    { a1[1] = 61; a1[2] = 37; printf "%d\n", a1[1] + a1[2] }
}
