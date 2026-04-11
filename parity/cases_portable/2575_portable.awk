# portable:2575
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((81 + 13) * 9 / 7)
    { x = "a2575b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
