# portable:2143
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((64 + 4) * 41 / 7)
    { x = "a2143b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
