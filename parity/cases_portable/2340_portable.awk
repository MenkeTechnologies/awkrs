# portable:2340
BEGIN {
    printf "%d\n", int(exp(log(85 + 1.0)))
    printf "%d\n", split("85:73:51", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
}
