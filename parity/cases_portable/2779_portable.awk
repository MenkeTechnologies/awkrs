# portable:2779
BEGIN {
    { a1[1] = 54; a1[2] = 84; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(54 + 1.0)))
    printf "%d\n", split("54:84:40", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
