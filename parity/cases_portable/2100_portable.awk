# portable:2100
BEGIN {
    printf "%d\n", int(exp(log(54 + 1.0)))
    printf "%d\n", split("54:68:78", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
}
