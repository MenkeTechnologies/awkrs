# portable:2701
BEGIN {
    printf "%s\n", sprintf("%02x", 15 + 10)
    printf "%s\n", tolower("X2701Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (90 < 49) + (49 < 55) * 2
}
