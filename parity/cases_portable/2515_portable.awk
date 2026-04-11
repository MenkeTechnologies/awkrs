# portable:2515
BEGIN {
    { a1[1] = 49; a1[2] = 34; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(49 + 1.0)))
    printf "%d\n", split("49:34:78", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
