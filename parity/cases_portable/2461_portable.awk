# portable:2461
BEGIN {
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2461Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (59 < 44) + (44 < 82) * 2
}
