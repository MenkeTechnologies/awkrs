# portable:2623
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((29 + 14) * 70 / 7)
    { x = "a2623b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
