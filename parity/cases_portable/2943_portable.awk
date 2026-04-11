# portable:2943
BEGIN {
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(34 * 34 + 38))
    printf "%d\n", length(sprintf("p%ddq", 2943))
    { x = "n2943n"; gsub(/n/, "m", x); printf "%s\n", x }
}
