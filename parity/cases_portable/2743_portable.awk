# portable:2743
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((93 + 61) * 15 / 7)
    { x = "a2743b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
