# portable:2892
BEGIN {
    printf "%d\n", int(exp(log(69 + 1.0)))
    printf "%d\n", split("69:40:47", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
}
