# portable:2108
BEGIN {
    { delete a2; a2["k"] = 19; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 0 + 10)
    printf "%s\n", tolower("X2108Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
