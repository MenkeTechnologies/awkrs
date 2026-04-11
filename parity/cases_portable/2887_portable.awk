# portable:2887
BEGIN {
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((34 + 64) * 32 / 7)
    { x = "a2887b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
