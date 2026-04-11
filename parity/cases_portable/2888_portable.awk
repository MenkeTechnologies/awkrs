# portable:2888
BEGIN {
    printf "%d\n", int(sqrt(35 * 35 + 41))
    printf "%d\n", length(sprintf("p%ddq", 2888))
    { x = "n2888n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
