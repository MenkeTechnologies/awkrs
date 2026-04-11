# portable:2791
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((41 + 62) * 76 / 7)
    { x = "a2791b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
