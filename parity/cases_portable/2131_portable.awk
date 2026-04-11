# portable:2131
BEGIN {
    { a1[1] = 77; a1[2] = 26; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(77 + 1.0)))
    printf "%d\n", split("77:26:5", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
}
