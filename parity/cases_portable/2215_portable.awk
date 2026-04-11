# portable:2215
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((83 + 50) * 8 / 7)
    { x = "a2215b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
