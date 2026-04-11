# portable:2137
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2137))
    { x = "n2137n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 2, 4)
}
