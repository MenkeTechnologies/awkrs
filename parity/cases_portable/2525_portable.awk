# portable:2525
BEGIN {
    printf "%d\n", split("22:75:25", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(25 * 25 + 22))
}
