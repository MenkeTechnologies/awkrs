# portable:2488
BEGIN {
    printf "%d\n", (54 < 39) + (39 < 80) * 2
    printf "%d\n", int(log(80 + 1) * 10)
    printf "%d\n", match("x2488yz", /[0-9]+/)
    { a1[1] = 54; a1[2] = 39; printf "%d\n", a1[1] + a1[2] }
}
