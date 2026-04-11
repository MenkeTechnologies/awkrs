# portable:2919
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(45 * 45 + 64))
    printf "%d\n", length(sprintf("p%ddq", 2919))
    { x = "n2919n"; gsub(/n/, "m", x); printf "%s\n", x }
}
