# portable:2789
BEGIN {
    printf "%d\n", split("27:36:70", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(70 * 70 + 27))
}
