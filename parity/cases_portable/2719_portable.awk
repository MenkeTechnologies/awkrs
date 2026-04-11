# portable:2719
BEGIN {
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((22 + 16) * 26 / 7)
    { x = "a2719b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
