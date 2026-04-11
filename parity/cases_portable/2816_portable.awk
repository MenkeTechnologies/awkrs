# portable:2816
BEGIN {
    printf "%d\n", int(sqrt(68 * 68 + 22))
    printf "%d\n", length(sprintf("p%ddq", 2816))
    { x = "n2816n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
