# portable:2947
BEGIN {
    { a1[1] = 66; a1[2] = 43; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(66 + 1.0)))
    printf "%d\n", split("66:43:46", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
