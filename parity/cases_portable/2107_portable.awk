# portable:2107
BEGIN {
    { a1[1] = 6; a1[2] = 70; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(6 + 1.0)))
    printf "%d\n", split("6:70:16", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
