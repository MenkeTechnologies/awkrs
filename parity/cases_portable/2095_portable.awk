# portable:2095
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((19 + 3) * 63 / 7)
    { x = "a2095b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
