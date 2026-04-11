# portable:2468
BEGIN {
    { delete a2; a2["k"] = 20; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2468Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
