# portable:2175
BEGIN {
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(54 * 54 + 94))
    printf "%d\n", length(sprintf("p%ddq", 2175))
    { x = "n2175n"; gsub(/n/, "m", x); printf "%s\n", x }
}
