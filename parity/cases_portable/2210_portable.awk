# portable:2210
BEGIN {
    printf "%d\n", match("x2210yz", /[0-9]+/)
    { a1[1] = 48; a1[2] = 74; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(48 + 1.0)))
    printf "%d\n", split("48:74:76", t, ":") + length(t[2])
}
