# portable:2275
BEGIN {
    { a1[1] = 18; a1[2] = 29; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(18 + 1.0)))
    printf "%d\n", split("18:29:22", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
