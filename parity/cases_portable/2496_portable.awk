# portable:2496
BEGIN {
    printf "%d\n", int((13 + 54) * 21 / 7)
    { x = "a2496b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
