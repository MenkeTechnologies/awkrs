# portable:2731
BEGIN {
    { a1[1] = 9; a1[2] = 83; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(9 + 1.0)))
    printf "%d\n", split("9:83:62", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
