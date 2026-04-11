# portable:2984
BEGIN {
    printf "%d\n", int(sqrt(74 * 74 + 34))
    printf "%d\n", length(sprintf("p%ddq", 2984))
    { x = "n2984n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
