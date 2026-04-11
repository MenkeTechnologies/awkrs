# portable:2821
BEGIN {
    printf "%s\n", sprintf("%02x", 16 + 10)
    printf "%s\n", tolower("X2821Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (57 < 7) + (7 < 83) * 2
}
