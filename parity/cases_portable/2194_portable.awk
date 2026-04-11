# portable:2194
BEGIN {
    printf "%.0f\n", sin(0) + cos(0) + 1
    printf "%d\n", index("alphabet", "h") + 1
    { delete a2; a2["k"] = 28; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 1 + 10)
}
