# portable:2258
BEGIN {
    printf "%d\n", match("x2258yz", /[0-9]+/)
    { a1[1] = 93; a1[2] = 75; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(93 + 1.0)))
    printf "%d\n", split("93:75:54", t, ":") + length(t[2])
}
