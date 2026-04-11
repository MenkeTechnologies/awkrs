# portable:2605
BEGIN {
    printf "%s\n", sprintf("%02x", 4 + 10)
    printf "%s\n", tolower("X2605Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (97 < 47) + (47 < 16) * 2
}
