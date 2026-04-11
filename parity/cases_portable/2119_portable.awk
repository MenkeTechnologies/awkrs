# portable:2119
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((90 + 48) * 52 / 7)
    { x = "a2119b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
