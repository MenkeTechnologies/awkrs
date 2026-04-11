# portable:2487
BEGIN {
    { s = 0; for (j = 1; j <= 2 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(77 * 77 + 47))
    printf "%d\n", length(sprintf("p%ddq", 2487))
    { x = "n2487n"; gsub(/n/, "m", x); printf "%s\n", x }
}
