# portable:2151
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(65 * 65 + 23))
    printf "%d\n", length(sprintf("p%ddq", 2151))
    { x = "n2151n"; gsub(/n/, "m", x); printf "%s\n", x }
}
