# portable:2799
BEGIN {
    { s = 0; for (j = 1; j <= 6 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(17 * 17 + 97))
    printf "%d\n", length(sprintf("p%ddq", 2799))
    { x = "n2799n"; gsub(/n/, "m", x); printf "%s\n", x }
}
