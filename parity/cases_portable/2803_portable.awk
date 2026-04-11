# portable:2803
BEGIN {
    { a1[1] = 28; a1[2] = 40; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(28 + 1.0)))
    printf "%d\n", split("28:40:29", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
