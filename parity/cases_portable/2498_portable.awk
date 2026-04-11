# portable:2498
BEGIN {
    printf "%d\n", match("x2498yz", /[0-9]+/)
    { a1[1] = 27; a1[2] = 80; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(27 + 1.0)))
    printf "%d\n", split("27:80:27", t, ":") + length(t[2])
}
