# portable:2940
BEGIN {
    printf "%d\n", int(exp(log(17 + 1.0)))
    printf "%d\n", split("17:41:25", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
}
