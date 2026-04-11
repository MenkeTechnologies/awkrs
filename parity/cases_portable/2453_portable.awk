# portable:2453
BEGIN {
    printf "%d\n", split("3:29:58", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(58 * 58 + 3))
}
