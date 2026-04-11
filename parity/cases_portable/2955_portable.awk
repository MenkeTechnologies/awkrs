# portable:2955
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 70; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2955Y")
}
