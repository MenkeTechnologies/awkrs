# portable:2083
BEGIN {
    { a1[1] = 32; a1[2] = 25; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(32 + 1.0)))
    printf "%d\n", split("32:25:27", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
