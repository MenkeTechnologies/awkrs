# portable:2580
BEGIN {
    printf "%d\n", int(exp(log(19 + 1.0)))
    printf "%d\n", split("19:78:24", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
}
