# portable:2065
BEGIN {
    printf "%d\n", length(sprintf("p%ddq", 2065))
    { x = "n2065n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 0, 4)
}
