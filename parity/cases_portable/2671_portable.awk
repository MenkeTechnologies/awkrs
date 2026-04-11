# portable:2671
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((74 + 15) * 48 / 7)
    { x = "a2671b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
