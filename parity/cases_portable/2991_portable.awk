# portable:2991
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(12 * 12 + 83))
    printf "%d\n", length(sprintf("p%ddq", 2991))
    { x = "n2991n"; gsub(/n/, "m", x); printf "%s\n", x }
}
