# portable:2739
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 3; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 2 + 10)
    printf "%s\n", tolower("X2739Y")
}
