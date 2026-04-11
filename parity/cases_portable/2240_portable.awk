# portable:2240
BEGIN {
    printf "%d\n", int(sqrt(83 * 83 + 64))
    printf "%d\n", length(sprintf("p%ddq", 2240))
    { x = "n2240n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
