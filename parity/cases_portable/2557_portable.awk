# portable:2557
BEGIN {
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2557Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (52 < 46) + (46 < 38) * 2
}
