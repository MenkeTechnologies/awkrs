# portable:2176
BEGIN {
    printf "%d\n", (4 < 77) + (77 < 57) * 2
    printf "%d\n", int(log(57 + 1) * 10)
    printf "%d\n", match("x2176yz", /[0-9]+/)
    { a1[1] = 4; a1[2] = 77; printf "%d\n", a1[1] + a1[2] }
}
