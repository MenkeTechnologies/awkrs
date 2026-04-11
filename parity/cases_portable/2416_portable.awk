# portable:2416
BEGIN {
    printf "%d\n", (35 < 82) + (82 < 30) * 2
    printf "%d\n", int(log(30 + 1) * 10)
    printf "%d\n", match("x2416yz", /[0-9]+/)
    { a1[1] = 35; a1[2] = 82; printf "%d\n", a1[1] + a1[2] }
}
