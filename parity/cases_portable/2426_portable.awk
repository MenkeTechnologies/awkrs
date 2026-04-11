# portable:2426
BEGIN {
    printf "%d\n", match("x2426yz", /[0-9]+/)
    { a1[1] = 8; a1[2] = 34; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(8 + 1.0)))
    printf "%d\n", split("8:34:60", t, ":") + length(t[2])
}
