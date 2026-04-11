# portable:2419
BEGIN {
    { a1[1] = 56; a1[2] = 32; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(56 + 1.0)))
    printf "%d\n", split("56:32:39", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
