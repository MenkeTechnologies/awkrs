# portable:2762
BEGIN {
    printf "%d\n", match("x2762yz", /[0-9]+/)
    { a1[1] = 32; a1[2] = 41; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(32 + 1.0)))
    printf "%d\n", split("32:41:72", t, ":") + length(t[2])
}
