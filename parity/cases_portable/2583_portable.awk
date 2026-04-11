# portable:2583
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(33 * 33 + 40))
    printf "%d\n", length(sprintf("p%ddq", 2583))
    { x = "n2583n"; gsub(/n/, "m", x); printf "%s\n", x }
}
