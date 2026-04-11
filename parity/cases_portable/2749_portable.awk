# portable:2749
BEGIN {
    printf "%s\n", sprintf("%02x", 12 + 10)
    printf "%s\n", tolower("X2749Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (38 < 50) + (50 < 33) * 2
}
