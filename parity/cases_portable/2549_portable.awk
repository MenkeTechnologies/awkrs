# portable:2549
BEGIN {
    printf "%d\n", split("93:31:14", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(14 * 14 + 93))
}
