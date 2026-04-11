# portable:2972
BEGIN {
    { delete a2; a2["k"] = 38; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2972Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
