# portable:2203
BEGIN {
    { a1[1] = 96; a1[2] = 72; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(96 + 1.0)))
    printf "%d\n", split("96:72:55", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
