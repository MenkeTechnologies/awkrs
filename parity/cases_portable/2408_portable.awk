# portable:2408
BEGIN {
    printf "%d\n", int(sqrt(6 * 6 + 76))
    printf "%d\n", length(sprintf("p%ddq", 2408))
    { x = "n2408n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
