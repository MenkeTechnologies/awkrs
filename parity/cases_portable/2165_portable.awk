# portable:2165
BEGIN {
    printf "%d\n", split("24:23:24", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(24 * 24 + 24))
}
