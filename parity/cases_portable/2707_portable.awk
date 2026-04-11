# portable:2707
BEGIN {
    { a1[1] = 35; a1[2] = 38; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(35 + 1.0)))
    printf "%d\n", split("35:38:73", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
