# portable:2048
BEGIN {
    printf "%d\n", int(sqrt(5 * 5 + 78))
    printf "%d\n", length(sprintf("p%ddq", 2048))
    { x = "n2048n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
