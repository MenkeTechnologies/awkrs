# portable:2656
BEGIN {
    printf "%d\n", (66 < 87) + (87 < 3) * 2
    printf "%d\n", int(log(3 + 1) * 10)
    printf "%d\n", match("x2656yz", /[0-9]+/)
    { a1[1] = 66; a1[2] = 87; printf "%d\n", a1[1] + a1[2] }
}
