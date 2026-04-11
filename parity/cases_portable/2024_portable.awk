# portable:2024
BEGIN {
    printf "%d\n", int(sqrt(16 * 16 + 7))
    printf "%d\n", length(sprintf("p%ddq", 2024))
    { x = "n2024n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
