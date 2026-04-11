# portable:2539
BEGIN {
    { a1[1] = 23; a1[2] = 79; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(23 + 1.0)))
    printf "%d\n", split("23:79:67", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
