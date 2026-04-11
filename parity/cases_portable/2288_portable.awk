# portable:2288
BEGIN {
    printf "%d\n", int(sqrt(61 * 61 + 12))
    printf "%d\n", length(sprintf("p%ddq", 2288))
    { x = "n2288n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
