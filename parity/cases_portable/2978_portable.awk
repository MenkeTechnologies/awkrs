# portable:2978
BEGIN {
    printf "%d\n", match("x2978yz", /[0-9]+/)
    { a1[1] = 89; a1[2] = 90; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(89 + 1.0)))
    printf "%d\n", split("89:90:56", t, ":") + length(t[2])
}
