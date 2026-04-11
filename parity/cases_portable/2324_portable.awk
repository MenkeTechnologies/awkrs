# portable:2324
BEGIN {
    { delete a2; a2["k"] = 3; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 12 + 10)
    printf "%s\n", tolower("X2324Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
