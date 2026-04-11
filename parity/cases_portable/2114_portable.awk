# portable:2114
BEGIN {
    printf "%d\n", match("x2114yz", /[0-9]+/)
    { a1[1] = 55; a1[2] = 72; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(55 + 1.0)))
    printf "%d\n", split("55:72:37", t, ":") + length(t[2])
}
