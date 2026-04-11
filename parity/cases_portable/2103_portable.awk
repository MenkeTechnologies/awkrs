# portable:2103
BEGIN {
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(4 * 4 + 75))
    printf "%d\n", length(sprintf("p%ddq", 2103))
    { x = "n2103n"; gsub(/n/, "m", x); printf "%s\n", x }
}
