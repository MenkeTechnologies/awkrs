# portable:2566
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(65 * 65 + 18))
    printf "%d\n", length(sprintf("p%ddq", 2566))
}
