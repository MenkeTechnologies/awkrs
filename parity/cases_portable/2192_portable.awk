# portable:2192
BEGIN {
    printf "%d\n", int(sqrt(22 * 22 + 19))
    printf "%d\n", length(sprintf("p%ddq", 2192))
    { x = "n2192n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
