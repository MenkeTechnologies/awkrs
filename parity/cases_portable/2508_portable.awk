# portable:2508
BEGIN {
    printf "%d\n", int(exp(log(97 + 1.0)))
    printf "%d\n", split("97:32:57", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
}
