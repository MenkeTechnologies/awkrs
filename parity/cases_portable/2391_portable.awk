# portable:2391
BEGIN {
    { s = 0; for (j = 1; j <= 4 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(38 * 38 + 54))
    printf "%d\n", length(sprintf("p%ddq", 2391))
    { x = "n2391n"; gsub(/n/, "m", x); printf "%s\n", x }
}
