# portable:2311
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((76 + 52) * 47 / 7)
    { x = "a2311b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
