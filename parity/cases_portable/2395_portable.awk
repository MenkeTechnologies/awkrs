# portable:2395
BEGIN {
    { a1[1] = 82; a1[2] = 76; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(82 + 1.0)))
    printf "%d\n", split("82:76:50", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
