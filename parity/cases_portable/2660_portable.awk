# portable:2660
BEGIN {
    { delete a2; a2["k"] = 15; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 8 + 10)
    printf "%s\n", tolower("X2660Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
