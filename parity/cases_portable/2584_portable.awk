# portable:2584
BEGIN {
    printf "%d\n", (47 < 41) + (41 < 36) * 2
    printf "%d\n", int(log(36 + 1) * 10)
    printf "%d\n", match("x2584yz", /[0-9]+/)
    { a1[1] = 47; a1[2] = 41; printf "%d\n", a1[1] + a1[2] }
}
