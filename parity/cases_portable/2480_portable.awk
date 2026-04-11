# portable:2480
BEGIN {
    printf "%d\n", int(sqrt(56 * 56 + 95))
    printf "%d\n", length(sprintf("p%ddq", 2480))
    { x = "n2480n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
