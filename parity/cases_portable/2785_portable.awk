# portable:2785
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2785))
    { x = "n2785n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 0, 4)
}
