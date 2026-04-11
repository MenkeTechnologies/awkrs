# portable:2060
BEGIN {
    { delete a2; a2["k"] = 41; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2060Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
