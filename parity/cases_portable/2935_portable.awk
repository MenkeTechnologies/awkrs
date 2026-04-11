# portable:2935
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((79 + 65) * 10 / 7)
    { x = "a2935b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
