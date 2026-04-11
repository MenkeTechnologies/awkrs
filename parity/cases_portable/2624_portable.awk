# portable:2624
BEGIN {
    printf "%d\n", int(sqrt(73 * 73 + 36))
    printf "%d\n", length(sprintf("p%ddq", 2624))
    { x = "n2624n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
