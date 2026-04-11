# portable:2401
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2401))
    { x = "n2401n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 1, 4)
}
