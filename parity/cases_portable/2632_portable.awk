# portable:2632
BEGIN {
    printf "%d\n", (92 < 42) + (42 < 14) * 2
    printf "%d\n", int(log(14 + 1) * 10)
    printf "%d\n", match("x2632yz", /[0-9]+/)
    { a1[1] = 92; a1[2] = 42; printf "%d\n", a1[1] + a1[2] }
}
