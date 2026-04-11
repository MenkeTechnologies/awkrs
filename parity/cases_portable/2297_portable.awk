# portable:2297
BEGIN {
    { x = "a2297b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
    printf "%.0f\n", sin(0) + cos(0) + 2
    printf "%d\n", index("alphabet", "h") + 2
    { delete a2; a2["k"] = 5; printf "%d\n", a2["k"] }
}
