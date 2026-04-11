# portable:2869
BEGIN {
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2869Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (5 < 8) + (8 < 61) * 2
}
