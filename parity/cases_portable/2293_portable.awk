# portable:2293
BEGIN {
    printf "%s\n", sprintf("%02x", 15 + 10)
    printf "%s\n", tolower("X2293Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (47 < 85) + (85 < 76) * 2
}
