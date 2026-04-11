# portable:2800
BEGIN {
    printf "%d\n", (7 < 90) + (90 < 20) * 2
    printf "%d\n", int(log(20 + 1) * 10)
    printf "%d\n", match("x2800yz", /[0-9]+/)
    { a1[1] = 7; a1[2] = 90; printf "%d\n", a1[1] + a1[2] }
}
