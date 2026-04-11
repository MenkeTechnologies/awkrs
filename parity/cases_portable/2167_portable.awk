# portable:2167
BEGIN {
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((38 + 49) * 30 / 7)
    { x = "a2167b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
