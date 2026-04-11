# portable:2247
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(21 * 21 + 16))
    printf "%d\n", length(sprintf("p%ddq", 2247))
    { x = "n2247n"; gsub(/n/, "m", x); printf "%s\n", x }
}
