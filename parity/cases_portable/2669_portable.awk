# portable:2669
BEGIN {
    printf "%d\n", split("60:78:42", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(42 * 42 + 60))
}
