# portable:2162
BEGIN {
    printf "%d\n", match("x2162yz", /[0-9]+/)
    { a1[1] = 3; a1[2] = 73; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(3 + 1.0)))
    printf "%d\n", split("3:73:15", t, ":") + length(t[2])
}
