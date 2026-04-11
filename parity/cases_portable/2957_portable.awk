# portable:2957
BEGIN {
    printf "%d\n", split("39:84:76", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(76 * 76 + 39))
}
