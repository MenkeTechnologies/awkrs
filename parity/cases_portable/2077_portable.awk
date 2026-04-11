# portable:2077
BEGIN {
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2077Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (87 < 36) + (36 < 9) * 2
}
