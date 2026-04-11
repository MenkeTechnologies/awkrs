# portable:2149
BEGIN {
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2149Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (9 < 82) + (82 < 59) * 2
}
