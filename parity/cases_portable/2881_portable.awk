# portable:2881
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2881))
    { x = "n2881n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 1, 4)
}
