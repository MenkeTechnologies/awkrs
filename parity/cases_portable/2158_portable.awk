# portable:2158
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(3 * 3 + 72))
    printf "%d\n", length(sprintf("p%ddq", 2158))
}
