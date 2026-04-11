# portable:2285
BEGIN {
    printf "%d\n", split("88:70:52", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(52 * 52 + 88))
}
