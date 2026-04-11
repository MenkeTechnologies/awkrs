# portable:2228
BEGIN {
    { delete a2; a2["k"] = 47; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 1 + 10)
    printf "%s\n", tolower("X2228Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
