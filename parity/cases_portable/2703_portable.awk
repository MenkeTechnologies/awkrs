# portable:2703
BEGIN {
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(61 * 61 + 7))
    printf "%d\n", length(sprintf("p%ddq", 2703))
    { x = "n2703n"; gsub(/n/, "m", x); printf "%s\n", x }
}
