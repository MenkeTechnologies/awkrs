# portable:2666
BEGIN {
    printf "%d\n", match("x2666yz", /[0-9]+/)
    { a1[1] = 39; a1[2] = 39; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(39 + 1.0)))
    printf "%d\n", split("39:39:33", t, ":") + length(t[2])
}
