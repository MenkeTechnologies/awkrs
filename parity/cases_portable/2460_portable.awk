# portable:2460
BEGIN {
    printf "%d\n", int(exp(log(52 + 1.0)))
    printf "%d\n", split("52:31:79", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
}
