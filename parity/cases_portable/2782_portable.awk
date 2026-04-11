# portable:2782
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(49 * 49 + 75))
    printf "%d\n", length(sprintf("p%ddq", 2782))
}
