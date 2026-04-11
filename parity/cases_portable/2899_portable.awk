# portable:2899
BEGIN {
    { a1[1] = 21; a1[2] = 42; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(21 + 1.0)))
    printf "%d\n", split("21:42:68", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
