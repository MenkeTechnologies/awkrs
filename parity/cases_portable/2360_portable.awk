# portable:2360
BEGIN {
    printf "%d\n", int(sqrt(28 * 28 + 31))
    printf "%d\n", length(sprintf("p%ddq", 2360))
    { x = "n2360n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
