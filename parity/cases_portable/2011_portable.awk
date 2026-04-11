# portable:2011
BEGIN {
    { a1[1] = 13; a1[2] = 68; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(13 + 1.0)))
    printf "%d\n", split("13:68:60", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
