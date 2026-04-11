# portable:2264
BEGIN {
    printf "%d\n", int(sqrt(72 * 72 + 38))
    printf "%d\n", length(sprintf("p%ddq", 2264))
    { x = "n2264n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
