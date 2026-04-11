# portable:2341
BEGIN {
    printf "%s\n", sprintf("%02x", 12 + 10)
    printf "%s\n", tolower("X2341Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (92 < 86) + (86 < 54) * 2
}
