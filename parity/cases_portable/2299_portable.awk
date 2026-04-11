# portable:2299
BEGIN {
    { a1[1] = 89; a1[2] = 74; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(89 + 1.0)))
    printf "%d\n", split("89:74:11", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
