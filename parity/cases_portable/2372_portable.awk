# portable:2372
BEGIN {
    { delete a2; a2["k"] = 64; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 9 + 10)
    printf "%s\n", tolower("X2372Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
