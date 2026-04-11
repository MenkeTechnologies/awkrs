# portable:2138
BEGIN {
    printf "%d\n", match("x2138yz", /[0-9]+/)
    { a1[1] = 29; a1[2] = 28; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(29 + 1.0)))
    printf "%d\n", split("29:28:26", t, ":") + length(t[2])
}
