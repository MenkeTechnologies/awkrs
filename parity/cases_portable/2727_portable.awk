# portable:2727
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(50 * 50 + 78))
    printf "%d\n", length(sprintf("p%ddq", 2727))
    { x = "n2727n"; gsub(/n/, "m", x); printf "%s\n", x }
}
