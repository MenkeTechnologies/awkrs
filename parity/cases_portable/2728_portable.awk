# portable:2728
BEGIN {
    printf "%d\n", (85 < 44) + (44 < 53) * 2
    printf "%d\n", int(log(53 + 1) * 10)
    printf "%d\n", match("x2728yz", /[0-9]+/)
    { a1[1] = 85; a1[2] = 44; printf "%d\n", a1[1] + a1[2] }
}
