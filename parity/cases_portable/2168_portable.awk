# portable:2168
BEGIN {
    printf "%d\n", int(sqrt(33 * 33 + 45))
    printf "%d\n", length(sprintf("p%ddq", 2168))
    { x = "n2168n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
