# portable:2287
BEGIN {
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((5 + 7) * 58 / 7)
    { x = "a2287b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
