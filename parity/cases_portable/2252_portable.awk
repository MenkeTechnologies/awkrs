# portable:2252
BEGIN {
    { delete a2; a2["k"] = 36; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 8 + 10)
    printf "%s\n", tolower("X2252Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
