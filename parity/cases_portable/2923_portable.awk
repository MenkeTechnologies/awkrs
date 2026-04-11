# portable:2923
BEGIN {
    { a1[1] = 92; a1[2] = 87; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(92 + 1.0)))
    printf "%d\n", split("92:87:57", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
