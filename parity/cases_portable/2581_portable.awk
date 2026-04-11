# portable:2581
BEGIN {
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2581Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (26 < 2) + (2 < 27) * 2
}
