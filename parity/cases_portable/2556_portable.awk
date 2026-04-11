# portable:2556
BEGIN {
    printf "%d\n", int(exp(log(45 + 1.0)))
    printf "%d\n", split("45:33:35", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
}
