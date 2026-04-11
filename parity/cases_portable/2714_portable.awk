# portable:2714
BEGIN {
    printf "%d\n", match("x2714yz", /[0-9]+/)
    { a1[1] = 84; a1[2] = 40; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(84 + 1.0)))
    printf "%d\n", split("84:40:11", t, ":") + length(t[2])
}
