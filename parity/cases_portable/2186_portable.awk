# portable:2186
BEGIN {
    printf "%d\n", match("x2186yz", /[0-9]+/)
    { a1[1] = 74; a1[2] = 29; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(74 + 1.0)))
    printf "%d\n", split("74:29:4", t, ":") + length(t[2])
}
