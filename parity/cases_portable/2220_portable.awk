# portable:2220
BEGIN {
    printf "%d\n", int(exp(log(21 + 1.0)))
    printf "%d\n", split("21:26:23", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
}
