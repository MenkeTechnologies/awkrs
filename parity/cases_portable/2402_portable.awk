# portable:2402
BEGIN {
    printf "%d\n", match("x2402yz", /[0-9]+/)
    { a1[1] = 34; a1[2] = 78; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(34 + 1.0)))
    printf "%d\n", split("34:78:71", t, ":") + length(t[2])
}
