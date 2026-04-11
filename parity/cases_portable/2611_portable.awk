# portable:2611
BEGIN {
    { a1[1] = 42; a1[2] = 36; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(42 + 1.0)))
    printf "%d\n", split("42:36:34", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
