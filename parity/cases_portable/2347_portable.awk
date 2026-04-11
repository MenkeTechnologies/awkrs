# portable:2347
BEGIN {
    { a1[1] = 37; a1[2] = 75; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(37 + 1.0)))
    printf "%d\n", split("37:75:72", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
