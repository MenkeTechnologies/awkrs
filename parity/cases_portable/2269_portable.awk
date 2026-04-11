# portable:2269
BEGIN {
    printf "%s\n", sprintf("%02x", 8 + 10)
    printf "%s\n", tolower("X2269Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (73 < 40) + (40 < 4) * 2
}
