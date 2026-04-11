# portable:2055
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(26 * 26 + 30))
    printf "%d\n", length(sprintf("p%ddq", 2055))
    { x = "n2055n"; gsub(/n/, "m", x); printf "%s\n", x }
}
