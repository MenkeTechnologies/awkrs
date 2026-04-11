# portable:2864
BEGIN {
    printf "%d\n", int(sqrt(46 * 46 + 67))
    printf "%d\n", length(sprintf("p%ddq", 2864))
    { x = "n2864n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
