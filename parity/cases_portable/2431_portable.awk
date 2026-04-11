# portable:2431
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((43 + 10) * 75 / 7)
    { x = "a2431b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
