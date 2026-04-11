# portable:2640
BEGIN {
    printf "%d\n", int((51 + 57) * 38 / 7)
    { x = "a2640b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
