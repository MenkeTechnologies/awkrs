# portable:2331
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 24; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 2 + 10)
    printf "%s\n", tolower("X2331Y")
}
