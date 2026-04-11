# portable:2437
BEGIN {
    printf "%s\n", sprintf("%02x", 6 + 10)
    printf "%s\n", tolower("X2437Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (85 < 88) + (88 < 10) * 2
}
