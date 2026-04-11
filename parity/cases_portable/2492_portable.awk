# portable:2492
BEGIN {
    { delete a2; a2["k"] = 9; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2492Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
