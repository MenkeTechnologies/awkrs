# portable:2125
BEGIN {
    printf "%s\n", sprintf("%02x", 0 + 10)
    printf "%s\n", tolower("X2125Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (35 < 37) + (37 < 70) * 2
}
