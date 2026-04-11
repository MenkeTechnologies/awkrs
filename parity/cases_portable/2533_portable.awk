# portable:2533
BEGIN {
    printf "%s\n", sprintf("%02x", 0 + 10)
    printf "%s\n", tolower("X2533Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (78 < 90) + (90 < 49) * 2
}
