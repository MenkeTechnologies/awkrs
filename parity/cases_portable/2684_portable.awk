# portable:2684
BEGIN {
    { delete a2; a2["k"] = 4; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 15 + 10)
    printf "%s\n", tolower("X2684Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
