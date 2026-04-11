# portable:2744
BEGIN {
    printf "%d\n", int(sqrt(18 * 18 + 3))
    printf "%d\n", length(sprintf("p%ddq", 2744))
    { x = "n2744n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
