# portable:2396
BEGIN {
    { delete a2; a2["k"] = 53; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 16 + 10)
    printf "%s\n", tolower("X2396Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
