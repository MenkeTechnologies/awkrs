# portable:2059
BEGIN {
    { a1[1] = 58; a1[2] = 69; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(58 + 1.0)))
    printf "%d\n", split("58:69:38", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
