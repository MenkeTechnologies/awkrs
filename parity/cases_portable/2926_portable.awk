# portable:2926
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(66 * 66 + 16))
    printf "%d\n", length(sprintf("p%ddq", 2926))
}
