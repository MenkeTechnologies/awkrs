# portable:2680
BEGIN {
    printf "%d\n", (40 < 43) + (43 < 75) * 2
    printf "%d\n", int(log(75 + 1) * 10)
    printf "%d\n", match("x2680yz", /[0-9]+/)
    { a1[1] = 40; a1[2] = 43; printf "%d\n", a1[1] + a1[2] }
}
