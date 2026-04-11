# portable:2576
BEGIN {
    printf "%d\n", int(sqrt(12 * 12 + 88))
    printf "%d\n", length(sprintf("p%ddq", 2576))
    { x = "n2576n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
