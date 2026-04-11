# portable:2173
BEGIN {
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2173Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (80 < 38) + (38 < 48) * 2
}
