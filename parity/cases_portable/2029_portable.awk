# portable:2029
BEGIN {
    printf "%s\n", sprintf("%02x", 6 + 10)
    printf "%s\n", tolower("X2029Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (42 < 35) + (35 < 31) * 2
}
