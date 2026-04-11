# portable:2216
BEGIN {
    printf "%d\n", int(sqrt(11 * 11 + 90))
    printf "%d\n", length(sprintf("p%ddq", 2216))
    { x = "n2216n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
