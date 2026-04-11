# portable:2677
BEGIN {
    printf "%s\n", sprintf("%02x", 8 + 10)
    printf "%s\n", tolower("X2677Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (19 < 4) + (4 < 66) * 2
}
