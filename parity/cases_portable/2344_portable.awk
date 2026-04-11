# portable:2344
BEGIN {
    printf "%d\n", (16 < 36) + (36 < 63) * 2
    printf "%d\n", int(log(63 + 1) * 10)
    printf "%d\n", match("x2344yz", /[0-9]+/)
    { a1[1] = 16; a1[2] = 36; printf "%d\n", a1[1] + a1[2] }
}
