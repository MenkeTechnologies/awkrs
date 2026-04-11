# portable:2954
BEGIN {
    printf "%d\n", match("x2954yz", /[0-9]+/)
    { a1[1] = 18; a1[2] = 45; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(18 + 1.0)))
    printf "%d\n", split("18:45:67", t, ":") + length(t[2])
}
