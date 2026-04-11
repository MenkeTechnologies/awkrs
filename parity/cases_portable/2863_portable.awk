# portable:2863
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((60 + 19) * 43 / 7)
    { x = "a2863b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
