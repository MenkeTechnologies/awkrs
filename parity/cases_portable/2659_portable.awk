# portable:2659
BEGIN {
    { a1[1] = 87; a1[2] = 37; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(87 + 1.0)))
    printf "%d\n", split("87:37:12", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
