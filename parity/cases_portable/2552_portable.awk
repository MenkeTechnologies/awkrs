# portable:2552
BEGIN {
    printf "%d\n", int(sqrt(23 * 23 + 17))
    printf "%d\n", length(sprintf("p%ddq", 2552))
    { x = "n2552n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
