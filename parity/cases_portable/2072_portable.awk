# portable:2072
BEGIN {
    printf "%d\n", int(sqrt(77 * 77 + 52))
    printf "%d\n", length(sprintf("p%ddq", 2072))
    { x = "n2072n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
