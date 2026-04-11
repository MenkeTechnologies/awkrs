# portable:2189
BEGIN {
    printf "%d\n", split("95:68:13", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(13 * 13 + 95))
}
