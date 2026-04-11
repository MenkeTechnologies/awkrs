# portable:2295
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(82 * 82 + 61))
    printf "%d\n", length(sprintf("p%ddq", 2295))
    { x = "n2295n"; gsub(/n/, "m", x); printf "%s\n", x }
}
