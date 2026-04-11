# portable:2093
BEGIN {
    printf "%d\n", split("5:66:57", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(57 * 57 + 5))
}
