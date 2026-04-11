# portable:2227
BEGIN {
    { a1[1] = 70; a1[2] = 28; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(70 + 1.0)))
    printf "%d\n", split("70:28:44", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
