# portable:2096
BEGIN {
    printf "%d\n", int(sqrt(66 * 66 + 26))
    printf "%d\n", length(sprintf("p%ddq", 2096))
    { x = "n2096n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
