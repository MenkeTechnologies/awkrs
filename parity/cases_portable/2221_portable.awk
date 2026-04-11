# portable:2221
BEGIN {
    printf "%s\n", sprintf("%02x", 11 + 10)
    printf "%s\n", tolower("X2221Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (28 < 39) + (39 < 26) * 2
}
