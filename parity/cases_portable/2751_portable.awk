# portable:2751
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(39 * 39 + 52))
    printf "%d\n", length(sprintf("p%ddq", 2751))
    { x = "n2751n"; gsub(/n/, "m", x); printf "%s\n", x }
}
