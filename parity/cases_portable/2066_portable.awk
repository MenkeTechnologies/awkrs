# portable:2066
BEGIN {
    printf "%d\n", match("x2066yz", /[0-9]+/)
    { a1[1] = 10; a1[2] = 71; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(10 + 1.0)))
    printf "%d\n", split("10:71:59", t, ":") + length(t[2])
}
