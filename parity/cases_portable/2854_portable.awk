# portable:2854
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(16 * 16 + 94))
    printf "%d\n", length(sprintf("p%ddq", 2854))
}
