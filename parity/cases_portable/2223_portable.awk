# portable:2223
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(32 * 32 + 42))
    printf "%d\n", length(sprintf("p%ddq", 2223))
    { x = "n2223n"; gsub(/n/, "m", x); printf "%s\n", x }
}
