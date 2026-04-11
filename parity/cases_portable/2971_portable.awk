# portable:2971
BEGIN {
    { a1[1] = 40; a1[2] = 88; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(40 + 1.0)))
    printf "%d\n", split("40:88:35", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
