# portable:2367
BEGIN {
    { s = 0; for (j = 1; j <= 1 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(49 * 49 + 80))
    printf "%d\n", length(sprintf("p%ddq", 2367))
    { x = "n2367n"; gsub(/n/, "m", x); printf "%s\n", x }
}
