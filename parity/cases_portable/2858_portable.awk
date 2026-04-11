# portable:2858
BEGIN {
    printf "%d\n", match("x2858yz", /[0-9]+/)
    { a1[1] = 25; a1[2] = 43; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(25 + 1.0)))
    printf "%d\n", split("25:43:28", t, ":") + length(t[2])
}
