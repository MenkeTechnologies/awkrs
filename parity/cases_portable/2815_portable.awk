# portable:2815
BEGIN {
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((15 + 18) * 65 / 7)
    { x = "a2815b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
