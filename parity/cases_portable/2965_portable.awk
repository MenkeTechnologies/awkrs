# portable:2965
BEGIN {
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2965Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (95 < 10) + (10 < 17) * 2
}
