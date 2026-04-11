# portable:2948
BEGIN {
    { delete a2; a2["k"] = 49; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2948Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
