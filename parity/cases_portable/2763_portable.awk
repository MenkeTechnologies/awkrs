# portable:2763
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 75; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 9 + 10)
    printf "%s\n", tolower("X2763Y")
}
