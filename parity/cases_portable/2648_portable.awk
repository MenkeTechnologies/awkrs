# portable:2648
BEGIN {
    printf "%d\n", int(sqrt(62 * 62 + 10))
    printf "%d\n", length(sprintf("p%ddq", 2648))
    { x = "n2648n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
