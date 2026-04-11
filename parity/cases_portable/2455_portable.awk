# portable:2455
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((17 + 55) * 64 / 7)
    { x = "a2455b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
