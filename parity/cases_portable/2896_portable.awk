# portable:2896
BEGIN {
    printf "%d\n", (97 < 3) + (3 < 59) * 2
    printf "%d\n", int(log(59 + 1) * 10)
    printf "%d\n", match("x2896yz", /[0-9]+/)
    { a1[1] = 97; a1[2] = 3; printf "%d\n", a1[1] + a1[2] }
}
