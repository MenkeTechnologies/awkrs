# portable:2599
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((55 + 58) * 81 / 7)
    { x = "a2599b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
