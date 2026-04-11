# portable:2381
BEGIN {
    printf "%d\n", split("81:72:8", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(8 * 8 + 81))
}
