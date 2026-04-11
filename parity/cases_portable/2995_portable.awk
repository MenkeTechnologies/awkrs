# portable:2995
BEGIN {
    { a1[1] = 14; a1[2] = 44; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(14 + 1.0)))
    printf "%d\n", split("14:44:24", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
