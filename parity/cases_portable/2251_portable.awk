# portable:2251
BEGIN {
    { a1[1] = 44; a1[2] = 73; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(44 + 1.0)))
    printf "%d\n", split("44:73:33", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
