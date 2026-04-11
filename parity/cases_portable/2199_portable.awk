# portable:2199
BEGIN {
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(43 * 43 + 68))
    printf "%d\n", length(sprintf("p%ddq", 2199))
    { x = "n2199n"; gsub(/n/, "m", x); printf "%s\n", x }
}
