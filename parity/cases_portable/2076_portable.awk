# portable:2076
BEGIN {
    printf "%d\n", int(exp(log(80 + 1.0)))
    printf "%d\n", split("80:23:6", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
}
