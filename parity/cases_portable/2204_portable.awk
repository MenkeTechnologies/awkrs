# portable:2204
BEGIN {
    { delete a2; a2["k"] = 58; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 11 + 10)
    printf "%s\n", tolower("X2204Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
