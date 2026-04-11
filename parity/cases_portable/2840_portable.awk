# portable:2840
BEGIN {
    printf "%d\n", int(sqrt(57 * 57 + 93))
    printf "%d\n", length(sprintf("p%ddq", 2840))
    { x = "n2840n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
