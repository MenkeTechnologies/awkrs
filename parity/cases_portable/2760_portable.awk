# portable:2760
BEGIN {
    printf "%d\n", int((18 + 15) * 66 / 7)
    { x = "a2760b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
