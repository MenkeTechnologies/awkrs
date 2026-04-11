# portable:2371
BEGIN {
    { a1[1] = 11; a1[2] = 31; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(11 + 1.0)))
    printf "%d\n", split("11:31:61", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
