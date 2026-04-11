# portable:2631
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(11 * 11 + 85))
    printf "%d\n", length(sprintf("p%ddq", 2631))
    { x = "n2631n"; gsub(/n/, "m", x); printf "%s\n", x }
}
