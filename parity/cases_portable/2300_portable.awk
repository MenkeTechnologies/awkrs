# portable:2300
BEGIN {
    { delete a2; a2["k"] = 14; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 5 + 10)
    printf "%s\n", tolower("X2300Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
