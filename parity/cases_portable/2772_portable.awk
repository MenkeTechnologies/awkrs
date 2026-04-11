# portable:2772
BEGIN {
    printf "%d\n", int(exp(log(5 + 1.0)))
    printf "%d\n", split("5:82:19", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
}
