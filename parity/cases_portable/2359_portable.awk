# portable:2359
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((24 + 53) * 25 / 7)
    { x = "a2359b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
