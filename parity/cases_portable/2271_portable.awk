# portable:2271
BEGIN {
    { s = 0; for (j = 1; j <= 3 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(10 * 10 + 87))
    printf "%d\n", length(sprintf("p%ddq", 2271))
    { x = "n2271n"; gsub(/n/, "m", x); printf "%s\n", x }
}
