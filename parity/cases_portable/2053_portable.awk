# portable:2053
BEGIN {
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2053Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (16 < 80) + (80 < 20) * 2
}
