# portable:2696
BEGIN {
    printf "%d\n", int(sqrt(40 * 40 + 55))
    printf "%d\n", length(sprintf("p%ddq", 2696))
    { x = "n2696n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
