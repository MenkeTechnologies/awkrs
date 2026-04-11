# portable:2786
BEGIN {
    printf "%d\n", match("x2786yz", /[0-9]+/)
    { a1[1] = 6; a1[2] = 86; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(6 + 1.0)))
    printf "%d\n", split("6:86:61", t, ":") + length(t[2])
}
