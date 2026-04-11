# portable:2120
BEGIN {
    printf "%d\n", int(sqrt(55 * 55 + 97))
    printf "%d\n", length(sprintf("p%ddq", 2120))
    { x = "n2120n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
