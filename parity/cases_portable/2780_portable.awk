# portable:2780
BEGIN {
    { delete a2; a2["k"] = 43; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 9 + 10)
    printf "%s\n", tolower("X2780Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
