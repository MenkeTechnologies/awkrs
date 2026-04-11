# portable:2672
BEGIN {
    printf "%d\n", int(sqrt(51 * 51 + 81))
    printf "%d\n", length(sprintf("p%ddq", 2672))
    { x = "n2672n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
