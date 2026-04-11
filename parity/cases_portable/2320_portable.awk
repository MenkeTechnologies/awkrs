# portable:2320
BEGIN {
    printf "%d\n", (42 < 80) + (80 < 74) * 2
    printf "%d\n", int(log(74 + 1) * 10)
    printf "%d\n", match("x2320yz", /[0-9]+/)
    { a1[1] = 42; a1[2] = 80; printf "%d\n", a1[1] + a1[2] }
}
