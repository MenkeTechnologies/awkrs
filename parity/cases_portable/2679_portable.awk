# portable:2679
BEGIN {
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(72 * 72 + 33))
    printf "%d\n", length(sprintf("p%ddq", 2679))
    { x = "n2679n"; gsub(/n/, "m", x); printf "%s\n", x }
}
