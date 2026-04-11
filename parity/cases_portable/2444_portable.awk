# portable:2444
BEGIN {
    { delete a2; a2["k"] = 31; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2444Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
