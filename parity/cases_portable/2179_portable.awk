# portable:2179
BEGIN {
    { a1[1] = 25; a1[2] = 27; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(25 + 1.0)))
    printf "%d\n", split("25:27:66", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
