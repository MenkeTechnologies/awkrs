# portable:2047
BEGIN {
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((71 + 2) * 85 / 7)
    { x = "a2047b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
