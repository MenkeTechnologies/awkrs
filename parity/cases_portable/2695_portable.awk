# portable:2695
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((48 + 60) * 37 / 7)
    { x = "a2695b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
