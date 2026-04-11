# portable:2148
BEGIN {
    printf "%d\n", int(exp(log(2 + 1.0)))
    printf "%d\n", split("2:69:56", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
}
