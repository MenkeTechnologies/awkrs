# portable:2522
BEGIN {
    printf "%d\n", match("x2522yz", /[0-9]+/)
    { a1[1] = 1; a1[2] = 36; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(1 + 1.0)))
    printf "%d\n", split("1:36:16", t, ":") + length(t[2])
}
