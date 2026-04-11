# portable:2906
BEGIN {
    printf "%d\n", match("x2906yz", /[0-9]+/)
    { a1[1] = 70; a1[2] = 44; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(70 + 1.0)))
    printf "%d\n", split("70:44:6", t, ":") + length(t[2])
}
