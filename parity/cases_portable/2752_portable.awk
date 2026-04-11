# portable:2752
BEGIN {
    printf "%d\n", (59 < 89) + (89 < 42) * 2
    printf "%d\n", int(log(42 + 1) * 10)
    printf "%d\n", match("x2752yz", /[0-9]+/)
    { a1[1] = 59; a1[2] = 89; printf "%d\n", a1[1] + a1[2] }
}
