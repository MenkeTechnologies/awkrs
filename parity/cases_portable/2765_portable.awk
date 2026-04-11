# portable:2765
BEGIN {
    printf "%d\n", split("53:80:81", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(81 * 81 + 53))
}
