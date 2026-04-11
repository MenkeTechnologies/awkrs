# portable:2551
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((10 + 57) * 20 / 7)
    { x = "a2551b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
