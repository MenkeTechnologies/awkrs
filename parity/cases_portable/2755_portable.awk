# portable:2755
BEGIN {
    { a1[1] = 80; a1[2] = 39; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(80 + 1.0)))
    printf "%d\n", split("80:39:51", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
