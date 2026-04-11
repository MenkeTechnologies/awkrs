# portable:2124
BEGIN {
    printf "%d\n", int(exp(log(28 + 1.0)))
    printf "%d\n", split("28:24:67", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
}
