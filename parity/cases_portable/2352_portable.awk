# portable:2352
BEGIN {
    printf "%d\n", int((72 + 51) * 4 / 7)
    { x = "a2352b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
