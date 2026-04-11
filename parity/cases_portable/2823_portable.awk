# portable:2823
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(6 * 6 + 71))
    printf "%d\n", length(sprintf("p%ddq", 2823))
    { x = "n2823n"; gsub(/n/, "m", x); printf "%s\n", x }
}
