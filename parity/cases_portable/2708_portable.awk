# portable:2708
BEGIN {
    { delete a2; a2["k"] = 76; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 5 + 10)
    printf "%s\n", tolower("X2708Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
