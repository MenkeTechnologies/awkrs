# portable:2248
BEGIN {
    printf "%d\n", (23 < 34) + (34 < 24) * 2
    printf "%d\n", int(log(24 + 1) * 10)
    printf "%d\n", match("x2248yz", /[0-9]+/)
    { a1[1] = 23; a1[2] = 34; printf "%d\n", a1[1] + a1[2] }
}
