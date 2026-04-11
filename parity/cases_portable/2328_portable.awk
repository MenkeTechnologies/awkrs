# portable:2328
BEGIN {
    printf "%d\n", int((1 + 6) * 15 / 7)
    { x = "a2328b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
