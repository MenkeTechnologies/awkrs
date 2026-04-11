# portable:2813
BEGIN {
    printf "%d\n", split("1:81:59", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(59 * 59 + 1))
}
