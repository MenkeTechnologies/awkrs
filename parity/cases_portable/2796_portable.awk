# portable:2796
BEGIN {
    printf "%d\n", int(exp(log(76 + 1.0)))
    printf "%d\n", split("76:38:8", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
}
