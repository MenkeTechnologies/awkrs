# portable:2329
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2329))
    { x = "n2329n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 4, 4)
}
