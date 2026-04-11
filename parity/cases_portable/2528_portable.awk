# portable:2528
BEGIN {
    printf "%d\n", int(sqrt(34 * 34 + 43))
    printf "%d\n", length(sprintf("p%ddq", 2528))
    { x = "n2528n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
