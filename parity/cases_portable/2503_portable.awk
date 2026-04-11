# portable:2503
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((62 + 56) * 42 / 7)
    { x = "a2503b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
