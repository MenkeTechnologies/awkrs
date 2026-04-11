# portable:2871
BEGIN {
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(67 * 67 + 19))
    printf "%d\n", length(sprintf("p%ddq", 2871))
    { x = "n2871n"; gsub(/n/, "m", x); printf "%s\n", x }
}
