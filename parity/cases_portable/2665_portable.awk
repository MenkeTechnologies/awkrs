# portable:2665
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2665))
    { x = "n2665n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 0, 4)
}
