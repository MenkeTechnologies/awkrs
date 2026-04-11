# portable:2776
BEGIN {
    printf "%d\n", (33 < 45) + (45 < 31) * 2
    printf "%d\n", int(log(31 + 1) * 10)
    printf "%d\n", match("x2776yz", /[0-9]+/)
    { a1[1] = 33; a1[2] = 45; printf "%d\n", a1[1] + a1[2] }
}
