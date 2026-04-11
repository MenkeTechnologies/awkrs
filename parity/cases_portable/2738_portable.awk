# portable:2738
BEGIN {
    printf "%d\n", match("x2738yz", /[0-9]+/)
    { a1[1] = 58; a1[2] = 85; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(58 + 1.0)))
    printf "%d\n", split("58:85:83", t, ":") + length(t[2])
}
