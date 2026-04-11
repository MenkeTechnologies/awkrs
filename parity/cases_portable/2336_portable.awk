# portable:2336
BEGIN {
    printf "%d\n", int(sqrt(39 * 39 + 57))
    printf "%d\n", length(sprintf("p%ddq", 2336))
    { x = "n2336n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
