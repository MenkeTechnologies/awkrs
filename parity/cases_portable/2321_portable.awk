# portable:2321
BEGIN {
    { x = "a2321b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 2
    printf "%d\n", index("alphabet", "h") + 2
    { delete a2; a2["k"] = 77; printf "%d\n", a2["k"] }
}
