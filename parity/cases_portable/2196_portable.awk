# portable:2196
BEGIN {
    printf "%d\n", int(exp(log(47 + 1.0)))
    printf "%d\n", split("47:70:34", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
}
