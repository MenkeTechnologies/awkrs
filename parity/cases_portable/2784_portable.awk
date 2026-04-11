# portable:2784
BEGIN {
    printf "%d\n", int((89 + 60) * 55 / 7)
    { x = "a2784b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 0
    printf "%d\n", index("alphabet", "h") + 0
}
