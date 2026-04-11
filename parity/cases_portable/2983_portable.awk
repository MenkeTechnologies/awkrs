# portable:2983
BEGIN {
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((27 + 66) * 71 / 7)
    { x = "a2983b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
