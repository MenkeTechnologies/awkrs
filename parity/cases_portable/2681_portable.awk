# portable:2681
BEGIN {
    { x = "a2681b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 2
    printf "%d\n", index("alphabet", "h") + 2
    { delete a2; a2["k"] = 78; printf "%d\n", a2["k"] }
}
