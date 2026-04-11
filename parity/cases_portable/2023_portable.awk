# portable:2023
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((97 + 46) * 13 / 7)
    { x = "a2023b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
