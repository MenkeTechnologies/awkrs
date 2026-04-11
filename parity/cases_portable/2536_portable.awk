# portable:2536
BEGIN {
    printf "%d\n", (2 < 40) + (40 < 58) * 2
    printf "%d\n", int(log(58 + 1) * 10)
    printf "%d\n", match("x2536yz", /[0-9]+/)
    { a1[1] = 2; a1[2] = 40; printf "%d\n", a1[1] + a1[2] }
}
