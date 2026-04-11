# portable:2612
BEGIN {
    { delete a2; a2["k"] = 37; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 11 + 10)
    printf "%s\n", tolower("X2612Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
