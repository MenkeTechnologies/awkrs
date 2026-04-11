# portable:2410
BEGIN {
    printf "%.0f\n", sin(0) + cos(0) + 1
    printf "%d\n", index("alphabet", "h") + 1
    { delete a2; a2["k"] = 12; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 13 + 10)
}
