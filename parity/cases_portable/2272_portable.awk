# portable:2272
BEGIN {
    printf "%d\n", (94 < 79) + (79 < 13) * 2
    printf "%d\n", int(log(13 + 1) * 10)
    printf "%d\n", match("x2272yz", /[0-9]+/)
    { a1[1] = 94; a1[2] = 79; printf "%d\n", a1[1] + a1[2] }
}
