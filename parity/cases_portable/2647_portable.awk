# portable:2647
BEGIN {
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((3 + 59) * 59 / 7)
    { x = "a2647b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
