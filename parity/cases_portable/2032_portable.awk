# portable:2032
BEGIN {
    printf "%d\n", (63 < 74) + (74 < 40) * 2
    printf "%d\n", int(log(40 + 1) * 10)
    printf "%d\n", match("x2032yz", /[0-9]+/)
    { a1[1] = 63; a1[2] = 74; printf "%d\n", a1[1] + a1[2] }
}
