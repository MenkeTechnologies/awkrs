# portable:2930
BEGIN {
    printf "%d\n", match("x2930yz", /[0-9]+/)
    { a1[1] = 44; a1[2] = 89; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(44 + 1.0)))
    printf "%d\n", split("44:89:78", t, ":") + length(t[2])
}
