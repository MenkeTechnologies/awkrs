# portable:2587
BEGIN {
    { a1[1] = 68; a1[2] = 80; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(68 + 1.0)))
    printf "%d\n", split("68:80:45", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
