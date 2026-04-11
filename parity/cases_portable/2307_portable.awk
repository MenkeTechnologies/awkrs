# portable:2307
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { delete a2; a2["k"] = 35; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 12 + 10)
    printf "%s\n", tolower("X2307Y")
}
