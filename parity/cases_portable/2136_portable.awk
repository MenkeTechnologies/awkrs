# portable:2136
BEGIN {
    printf "%d\n", int((15 + 2) * 20 / 7)
    { x = "a2136b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
