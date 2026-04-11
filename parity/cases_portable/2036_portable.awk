# portable:2036
BEGIN {
    { delete a2; a2["k"] = 52; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2036Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
