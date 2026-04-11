# portable:2827
BEGIN {
    { a1[1] = 2; a1[2] = 85; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(2 + 1.0)))
    printf "%d\n", split("2:85:18", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
