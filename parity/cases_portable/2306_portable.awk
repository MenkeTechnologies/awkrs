# portable:2306
BEGIN {
    printf "%d\n", match("x2306yz", /[0-9]+/)
    { a1[1] = 41; a1[2] = 76; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(41 + 1.0)))
    printf "%d\n", split("41:76:32", t, ":") + length(t[2])
}
