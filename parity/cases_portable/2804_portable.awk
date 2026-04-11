# portable:2804
BEGIN {
    { delete a2; a2["k"] = 32; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 16 + 10)
    printf "%s\n", tolower("X2804Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
