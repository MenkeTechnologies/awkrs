# portable:2071
BEGIN {
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((45 + 47) * 74 / 7)
    { x = "a2071b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 1
}
