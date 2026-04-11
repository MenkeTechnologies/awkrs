# portable:2559
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(44 * 44 + 66))
    printf "%d\n", length(sprintf("p%ddq", 2559))
    { x = "n2559n"; gsub(/n/, "m", x); printf "%s\n", x }
}
