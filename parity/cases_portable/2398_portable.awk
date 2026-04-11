# portable:2398
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(59 * 59 + 6))
    printf "%d\n", length(sprintf("p%ddq", 2398))
}
