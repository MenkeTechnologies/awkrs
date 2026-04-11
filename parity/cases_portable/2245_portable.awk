# portable:2245
BEGIN {
    printf "%s\n", sprintf("%02x", 1 + 10)
    printf "%s\n", tolower("X2245Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (2 < 84) + (84 < 15) * 2
}
