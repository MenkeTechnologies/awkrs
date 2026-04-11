# portable:2992
BEGIN {
    printf "%d\n", (90 < 5) + (5 < 15) * 2
    printf "%d\n", int(log(15 + 1) * 10)
    printf "%d\n", match("x2992yz", /[0-9]+/)
    { a1[1] = 90; a1[2] = 5; printf "%d\n", a1[1] + a1[2] }
}
