# portable:2436
BEGIN {
    printf "%d\n", int(exp(log(78 + 1.0)))
    printf "%d\n", split("78:75:7", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
}
