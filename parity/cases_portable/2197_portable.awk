# portable:2197
BEGIN {
    printf "%s\n", sprintf("%02x", 4 + 10)
    printf "%s\n", tolower("X2197Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (54 < 83) + (83 < 37) * 2
}
