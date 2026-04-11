# portable:2239
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((57 + 6) * 80 / 7)
    { x = "a2239b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
