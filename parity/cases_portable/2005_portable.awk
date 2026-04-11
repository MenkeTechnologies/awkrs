# portable:2005
BEGIN {
    printf "%s\n", sprintf("%02x", 16 + 10)
    printf "%s\n", tolower("X2005Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (68 < 79) + (79 < 42) * 2
}
