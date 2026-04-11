# portable:2638
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(32 * 32 + 37))
    printf "%d\n", length(sprintf("p%ddq", 2638))
}
