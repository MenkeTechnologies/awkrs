# portable:2090
BEGIN {
    printf "%d\n", match("x2090yz", /[0-9]+/)
    { a1[1] = 81; a1[2] = 27; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(81 + 1.0)))
    printf "%d\n", split("81:27:48", t, ":") + length(t[2])
}
