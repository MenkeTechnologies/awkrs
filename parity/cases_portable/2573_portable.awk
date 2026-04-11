# portable:2573
BEGIN {
    printf "%d\n", split("67:76:3", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(3 * 3 + 67))
}
