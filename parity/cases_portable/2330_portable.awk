# portable:2330
BEGIN {
    printf "%d\n", match("x2330yz", /[0-9]+/)
    { a1[1] = 15; a1[2] = 32; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(15 + 1.0)))
    printf "%d\n", split("15:32:21", t, ":") + length(t[2])
}
