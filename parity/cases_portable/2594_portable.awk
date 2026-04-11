# portable:2594
BEGIN {
    printf "%d\n", match("x2594yz", /[0-9]+/)
    { a1[1] = 20; a1[2] = 82; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(20 + 1.0)))
    printf "%d\n", split("20:82:66", t, ":") + length(t[2])
}
