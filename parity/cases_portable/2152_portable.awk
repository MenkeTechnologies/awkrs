# portable:2152
BEGIN {
    printf "%d\n", (30 < 32) + (32 < 68) * 2
    printf "%d\n", int(log(68 + 1) * 10)
    printf "%d\n", match("x2152yz", /[0-9]+/)
    { a1[1] = 30; a1[2] = 32; printf "%d\n", a1[1] + a1[2] }
}
