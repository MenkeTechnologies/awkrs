# portable:2959
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((53 + 21) * 82 / 7)
    { x = "a2959b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
