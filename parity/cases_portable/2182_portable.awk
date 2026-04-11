# portable:2182
BEGIN {
    { u = ""; printf "%d\n", (u == "") + 1 }
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(75 * 75 + 46))
    printf "%d\n", length(sprintf("p%ddq", 2182))
}
