# portable:2953
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2953))
    { x = "n2953n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 3, 4)
}
