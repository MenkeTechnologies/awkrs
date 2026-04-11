# portable:2415
BEGIN {
    { s = 0; for (j = 1; j <= 0 + 1; j++) s += j; printf "%d\n", s }
    printf "%d\n", int(sqrt(27 * 27 + 28))
    printf "%d\n", length(sprintf("p%ddq", 2415))
    { x = "n2415n"; gsub(/n/, "m", x); printf "%s\n", x }
}
