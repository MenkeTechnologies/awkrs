# portable:2450
BEGIN {
    printf "%d\n", match("x2450yz", /[0-9]+/)
    { a1[1] = 79; a1[2] = 79; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(79 + 1.0)))
    printf "%d\n", split("79:79:49", t, ":") + length(t[2])
}
