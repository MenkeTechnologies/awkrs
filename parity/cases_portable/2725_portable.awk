# portable:2725
BEGIN {
    printf "%s\n", sprintf("%02x", 5 + 10)
    printf "%s\n", tolower("X2725Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (64 < 5) + (5 < 44) * 2
}
