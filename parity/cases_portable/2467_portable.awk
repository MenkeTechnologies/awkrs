# portable:2467
BEGIN {
    { a1[1] = 4; a1[2] = 33; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(4 + 1.0)))
    printf "%d\n", split("4:33:17", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
