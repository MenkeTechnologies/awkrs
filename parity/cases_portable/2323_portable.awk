# portable:2323
BEGIN {
    { a1[1] = 63; a1[2] = 30; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(63 + 1.0)))
    printf "%d\n", split("63:30:83", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
