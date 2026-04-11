# portable:2588
BEGIN {
    { delete a2; a2["k"] = 48; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 4 + 10)
    printf "%s\n", tolower("X2588Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
