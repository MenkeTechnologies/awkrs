# portable:2389
BEGIN {
    printf "%s\n", sprintf("%02x", 9 + 10)
    printf "%s\n", tolower("X2389Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (40 < 87) + (87 < 32) * 2
}
