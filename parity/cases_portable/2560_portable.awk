# portable:2560
BEGIN {
    printf "%d\n", (73 < 85) + (85 < 47) * 2
    printf "%d\n", int(log(47 + 1) * 10)
    printf "%d\n", match("x2560yz", /[0-9]+/)
    { a1[1] = 73; a1[2] = 85; printf "%d\n", a1[1] + a1[2] }
}
