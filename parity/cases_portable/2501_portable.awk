# portable:2501
BEGIN {
    printf "%d\n", split("48:30:36", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(36 * 36 + 48))
}
