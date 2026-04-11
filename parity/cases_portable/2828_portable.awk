# portable:2828
BEGIN {
    { delete a2; a2["k"] = 21; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 6 + 10)
    printf "%s\n", tolower("X2828Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
