# portable:2127
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(76 * 76 + 49))
    printf "%d\n", length(sprintf("p%ddq", 2127))
    { x = "n2127n"; gsub(/n/, "m", x); printf "%s\n", x }
}
