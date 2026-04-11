# portable:2233
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2233))
    { x = "n2233n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 3, 4)
}
