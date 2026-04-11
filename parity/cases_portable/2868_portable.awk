# portable:2868
BEGIN {
    printf "%d\n", int(exp(log(95 + 1.0)))
    printf "%d\n", split("95:84:58", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
}
