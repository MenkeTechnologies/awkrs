# portable:2607
BEGIN {
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(22 * 22 + 14))
    printf "%d\n", length(sprintf("p%ddq", 2607))
    { x = "n2607n"; gsub(/n/, "m", x); printf "%s\n", x }
}
