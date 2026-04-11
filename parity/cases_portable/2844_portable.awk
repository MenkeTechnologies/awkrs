# portable:2844
BEGIN {
    printf "%d\n", int(exp(log(24 + 1.0)))
    printf "%d\n", split("24:39:69", t, ":") + length(t[2])
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
}
