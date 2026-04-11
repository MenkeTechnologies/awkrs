# portable:2797
BEGIN {
    printf "%s\n", sprintf("%02x", 9 + 10)
    printf "%s\n", tolower("X2797Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (83 < 51) + (51 < 11) * 2
}
