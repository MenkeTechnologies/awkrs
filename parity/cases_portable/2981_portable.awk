# portable:2981
BEGIN {
    printf "%d\n", split("13:40:65", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(65 * 65 + 13))
}
