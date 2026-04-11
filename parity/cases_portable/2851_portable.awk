# portable:2851
BEGIN {
    { a1[1] = 73; a1[2] = 41; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(73 + 1.0)))
    printf "%d\n", split("73:41:7", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
