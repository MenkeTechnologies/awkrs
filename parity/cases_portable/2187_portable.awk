# portable:2187
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 7; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 11 + 10)
    printf "%s\n", tolower("X2187Y")
}
