# portable:2463
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(5 * 5 + 73))
    printf "%d\n", length(sprintf("p%ddq", 2463))
    { x = "n2463n"; gsub(/n/, "m", x); printf "%s\n", x }
}
