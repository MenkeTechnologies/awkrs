# portable:2775
BEGIN {
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(28 * 28 + 26))
    printf "%d\n", length(sprintf("p%ddq", 2775))
    { x = "n2775n"; gsub(/n/, "m", x); printf "%s\n", x }
}
