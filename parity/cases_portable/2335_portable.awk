# portable:2335
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((50 + 8) * 36 / 7)
    { x = "a2335b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
