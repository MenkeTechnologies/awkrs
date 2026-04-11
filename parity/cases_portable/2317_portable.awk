# portable:2317
BEGIN {
    printf "%s\n", sprintf("%02x", 5 + 10)
    printf "%s\n", tolower("X2317Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (21 < 41) + (41 < 65) * 2
}
