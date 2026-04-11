# portable:2101
BEGIN {
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2101Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (61 < 81) + (81 < 81) * 2
}
