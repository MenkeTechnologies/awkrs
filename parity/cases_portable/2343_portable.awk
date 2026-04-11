# portable:2343
BEGIN {
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(60 * 60 + 9))
    printf "%d\n", length(sprintf("p%ddq", 2343))
    { x = "n2343n"; gsub(/n/, "m", x); printf "%s\n", x }
}
