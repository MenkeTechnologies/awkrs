# portable:2042
BEGIN {
    printf "%d\n", match("x2042yz", /[0-9]+/)
    { a1[1] = 36; a1[2] = 26; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(36 + 1.0)))
    printf "%d\n", split("36:26:70", t, ":") + length(t[2])
}
