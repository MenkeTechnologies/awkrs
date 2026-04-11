# portable:2155
BEGIN {
    { a1[1] = 51; a1[2] = 71; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(51 + 1.0)))
    printf "%d\n", split("51:71:77", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
