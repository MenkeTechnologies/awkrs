# portable:2810
BEGIN {
    printf "%d\n", match("x2810yz", /[0-9]+/)
    { a1[1] = 77; a1[2] = 42; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(77 + 1.0)))
    printf "%d\n", split("77:42:50", t, ":") + length(t[2])
}
