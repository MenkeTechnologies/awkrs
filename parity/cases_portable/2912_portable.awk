# portable:2912
BEGIN {
    printf "%d\n", int(sqrt(24 * 24 + 15))
    printf "%d\n", length(sprintf("p%ddq", 2912))
    { x = "n2912n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
