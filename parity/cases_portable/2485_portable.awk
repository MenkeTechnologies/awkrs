# portable:2485
BEGIN {
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2485Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (33 < 89) + (89 < 71) * 2
}
