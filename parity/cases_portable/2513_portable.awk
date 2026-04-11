# portable:2513
BEGIN {
    { x = "a2513b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 2
    printf "%d\n", index("alphabet", "h") + 2
    { delete a2; a2["k"] = 72; printf "%d\n", a2["k"] }
}
