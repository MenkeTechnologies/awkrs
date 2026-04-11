# portable:2834
BEGIN {
    printf "%d\n", match("x2834yz", /[0-9]+/)
    { a1[1] = 51; a1[2] = 87; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(51 + 1.0)))
    printf "%d\n", split("51:87:39", t, ":") + length(t[2])
}
