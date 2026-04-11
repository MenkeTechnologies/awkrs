# portable:2635
BEGIN {
    { a1[1] = 16; a1[2] = 81; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(16 + 1.0)))
    printf "%d\n", split("16:81:23", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
