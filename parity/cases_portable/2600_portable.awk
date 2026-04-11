# portable:2600
BEGIN {
    printf "%d\n", int(sqrt(84 * 84 + 62))
    printf "%d\n", length(sprintf("p%ddq", 2600))
    { x = "n2600n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
