# portable:2768
BEGIN {
    printf "%d\n", int(sqrt(7 * 7 + 74))
    printf "%d\n", length(sprintf("p%ddq", 2768))
    { x = "n2768n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
