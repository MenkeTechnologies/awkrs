# portable:2354
BEGIN {
    printf "%d\n", match("x2354yz", /[0-9]+/)
    { a1[1] = 86; a1[2] = 77; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(86 + 1.0)))
    printf "%d\n", split("86:77:10", t, ":") + length(t[2])
}
