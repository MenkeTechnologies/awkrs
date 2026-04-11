# portable:2319
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(71 * 71 + 35))
    printf "%d\n", length(sprintf("p%ddq", 2319))
    { x = "n2319n"; gsub(/n/, "m", x); printf "%s\n", x }
}
