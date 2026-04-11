# portable:2491
BEGIN {
    { a1[1] = 75; a1[2] = 78; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(75 + 1.0)))
    printf "%d\n", split("75:78:6", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
