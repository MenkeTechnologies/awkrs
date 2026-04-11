# portable:2546
BEGIN {
    printf "%d\n", match("x2546yz", /[0-9]+/)
    { a1[1] = 72; a1[2] = 81; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(72 + 1.0)))
    printf "%d\n", split("72:81:5", t, ":") + length(t[2])
}
