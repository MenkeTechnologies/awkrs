# portable:2839
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((86 + 63) * 54 / 7)
    { x = "a2839b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
