# portable:2378
BEGIN {
    printf "%d\n", match("x2378yz", /[0-9]+/)
    { a1[1] = 60; a1[2] = 33; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(60 + 1.0)))
    printf "%d\n", split("60:33:82", t, ":") + length(t[2])
}
