# portable:2845
BEGIN {
    printf "%s\n", sprintf("%02x", 6 + 10)
    printf "%s\n", tolower("X2845Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (31 < 52) + (52 < 72) * 2
}
