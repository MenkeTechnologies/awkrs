# portable:2456
BEGIN {
    printf "%d\n", int(sqrt(67 * 67 + 24))
    printf "%d\n", length(sprintf("p%ddq", 2456))
    { x = "n2456n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
