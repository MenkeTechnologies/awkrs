# portable:2312
BEGIN {
    printf "%d\n", int(sqrt(50 * 50 + 83))
    printf "%d\n", length(sprintf("p%ddq", 2312))
    { x = "n2312n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
