# portable:2875
BEGIN {
    { a1[1] = 47; a1[2] = 86; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(47 + 1.0)))
    printf "%d\n", split("47:86:79", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
