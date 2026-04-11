# portable:2964
BEGIN {
    printf "%d\n", int(exp(log(88 + 1.0)))
    printf "%d\n", split("88:86:14", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
}
