# portable:2885
BEGIN {
    printf "%d\n", split("20:38:26", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(26 * 26 + 20))
}
