# portable:2967
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(23 * 23 + 12))
    printf "%d\n", length(sprintf("p%ddq", 2967))
    { x = "n2967n"; gsub(/n/, "m", x); printf "%s\n", x }
}
