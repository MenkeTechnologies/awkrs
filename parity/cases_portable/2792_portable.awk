# portable:2792
BEGIN {
    printf "%d\n", int(sqrt(79 * 79 + 48))
    printf "%d\n", length(sprintf("p%ddq", 2792))
    { x = "n2792n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
