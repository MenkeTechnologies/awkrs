# portable:2563
BEGIN {
    { a1[1] = 94; a1[2] = 35; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(94 + 1.0)))
    printf "%d\n", split("94:35:56", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
