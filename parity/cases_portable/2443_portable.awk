# portable:2443
BEGIN {
    { a1[1] = 30; a1[2] = 77; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(30 + 1.0)))
    printf "%d\n", split("30:77:28", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
