# portable:2474
BEGIN {
    printf "%d\n", match("x2474yz", /[0-9]+/)
    { a1[1] = 53; a1[2] = 35; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(53 + 1.0)))
    printf "%d\n", split("53:35:38", t, ":") + length(t[2])
}
