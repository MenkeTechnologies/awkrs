# portable:2511
BEGIN {
    { s = 0; for (j = 1; j <= 5 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(66 * 66 + 21))
    printf "%d\n", length(sprintf("p%ddq", 2511))
    { x = "n2511n"; gsub(/n/, "m", x); printf "%s\n", x }
}
