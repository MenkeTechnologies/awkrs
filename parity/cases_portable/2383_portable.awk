# portable:2383
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((95 + 9) * 14 / 7)
    { x = "a2383b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
