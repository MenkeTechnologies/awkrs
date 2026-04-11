# portable:2364
BEGIN {
    printf "%d\n", int(exp(log(59 + 1.0)))
    printf "%d\n", split("59:29:40", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
}
