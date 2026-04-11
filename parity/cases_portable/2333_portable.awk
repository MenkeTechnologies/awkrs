# portable:2333
BEGIN {
    printf "%d\n", split("36:71:30", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(30 * 30 + 36))
}
