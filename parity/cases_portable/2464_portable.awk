# portable:2464
BEGIN {
    printf "%d\n", (80 < 83) + (83 < 8) * 2
    printf "%d\n", int(log(8 + 1) * 10)
    printf "%d\n", match("x2464yz", /[0-9]+/)
    { a1[1] = 80; a1[2] = 83; printf "%d\n", a1[1] + a1[2] }
}
