# portable:2861
BEGIN {
    printf "%d\n", split("46:82:37", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(37 * 37 + 46))
}
