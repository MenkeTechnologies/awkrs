# portable:2882
BEGIN {
    printf "%d\n", match("x2882yz", /[0-9]+/)
    { a1[1] = 96; a1[2] = 88; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(96 + 1.0)))
    printf "%d\n", split("96:88:17", t, ":") + length(t[2])
}
