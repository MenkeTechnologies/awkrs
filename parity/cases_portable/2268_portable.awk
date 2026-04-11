# portable:2268
BEGIN {
    printf "%d\n", int(exp(log(66 + 1.0)))
    printf "%d\n", split("66:27:84", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
}
