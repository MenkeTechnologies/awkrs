# portable:2761
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2761))
    { x = "n2761n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 1, 4)
}
