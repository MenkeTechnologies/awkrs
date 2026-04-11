# portable:2895
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(56 * 56 + 90))
    printf "%d\n", length(sprintf("p%ddq", 2895))
    { x = "n2895n"; gsub(/n/, "m", x); printf "%s\n", x }
}
