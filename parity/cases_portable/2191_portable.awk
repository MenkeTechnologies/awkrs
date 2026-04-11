# portable:2191
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((12 + 5) * 19 / 7)
    { x = "a2191b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
