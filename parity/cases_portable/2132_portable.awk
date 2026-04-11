# portable:2132
BEGIN {
    { delete a2; a2["k"] = 8; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2132Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
