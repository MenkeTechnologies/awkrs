# portable:2368
BEGIN {
    printf "%d\n", (87 < 81) + (81 < 52) * 2
    printf "%d\n", int(log(52 + 1) * 10)
    printf "%d\n", match("x2368yz", /[0-9]+/)
    { a1[1] = 87; a1[2] = 81; printf "%d\n", a1[1] + a1[2] }
}
