# portable:2618
BEGIN {
    printf "%d\n", match("x2618yz", /[0-9]+/)
    { a1[1] = 91; a1[2] = 38; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(91 + 1.0)))
    printf "%d\n", split("91:38:55", t, ":") + length(t[2])
}
