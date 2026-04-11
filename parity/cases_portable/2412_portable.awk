# portable:2412
BEGIN {
    printf "%d\n", int(exp(log(7 + 1.0)))
    printf "%d\n", split("7:30:18", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
}
