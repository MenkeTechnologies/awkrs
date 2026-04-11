# portable:2653
BEGIN {
    printf "%s\n", sprintf("%02x", 1 + 10)
    printf "%s\n", tolower("X2653Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (45 < 48) + (48 < 77) * 2
}
