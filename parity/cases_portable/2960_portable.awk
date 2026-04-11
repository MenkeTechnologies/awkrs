# portable:2960
BEGIN {
    printf "%d\n", int(sqrt(85 * 85 + 60))
    printf "%d\n", length(sprintf("p%ddq", 2960))
    { x = "n2960n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
