# portable:2690
BEGIN {
    printf "%d\n", match("x2690yz", /[0-9]+/)
    { a1[1] = 13; a1[2] = 84; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(13 + 1.0)))
    printf "%d\n", split("13:84:22", t, ":") + length(t[2])
}
