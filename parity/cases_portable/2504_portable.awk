# portable:2504
BEGIN {
    printf "%d\n", int(sqrt(45 * 45 + 69))
    printf "%d\n", length(sprintf("p%ddq", 2504))
    { x = "n2504n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
