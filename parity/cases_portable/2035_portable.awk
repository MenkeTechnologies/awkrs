# portable:2035
BEGIN {
    { a1[1] = 84; a1[2] = 24; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(84 + 1.0)))
    printf "%d\n", split("84:24:49", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
