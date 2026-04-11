# portable:2911
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((8 + 20) * 21 / 7)
    { x = "a2911b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
