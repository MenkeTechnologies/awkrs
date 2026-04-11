# portable:2655
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(83 * 83 + 59))
    printf "%d\n", length(sprintf("p%ddq", 2655))
    { x = "n2655n"; gsub(/n/, "m", x); printf "%s\n", x }
}
