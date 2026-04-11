# portable:2917
BEGIN {
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2917Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (50 < 9) + (9 < 39) * 2
}
