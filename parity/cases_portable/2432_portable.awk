# portable:2432
BEGIN {
    printf "%d\n", int(sqrt(78 * 78 + 50))
    printf "%d\n", length(sprintf("p%ddq", 2432))
    { x = "n2432n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
}
