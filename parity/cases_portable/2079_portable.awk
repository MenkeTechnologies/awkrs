# portable:2079
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(15 * 15 + 4))
    printf "%d\n", length(sprintf("p%ddq", 2079))
    { x = "n2079n"; gsub(/n/, "m", x); printf "%s\n", x }
}
