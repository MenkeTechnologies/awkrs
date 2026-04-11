# portable:2683
BEGIN {
    { a1[1] = 61; a1[2] = 82; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(61 + 1.0)))
    printf "%d\n", split("61:82:84", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
