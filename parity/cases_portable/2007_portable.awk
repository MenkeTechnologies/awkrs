# portable:2007
BEGIN {
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(48 * 48 + 82))
    printf "%d\n", length(sprintf("p%ddq", 2007))
    { x = "n2007n"; gsub(/n/, "m", x); printf "%s\n", x }
}
