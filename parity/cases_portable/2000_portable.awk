# portable:2000
BEGIN {
    printf "%d\n", int(sqrt(27 * 27 + 33))
    printf "%d\n", length(sprintf("p%ddq", 2000))
    { x = "n2000n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
