# portable:2384
BEGIN {
    printf "%d\n", int(sqrt(17 * 17 + 5))
    printf "%d\n", length(sprintf("p%ddq", 2384))
    { x = "n2384n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
