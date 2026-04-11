# portable:2234
BEGIN {
    printf "%d\n", match("x2234yz", /[0-9]+/)
    { a1[1] = 22; a1[2] = 30; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(22 + 1.0)))
    printf "%d\n", split("22:30:65", t, ":") + length(t[2])
}
