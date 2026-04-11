# portable:2629
BEGIN {
    printf "%s\n", sprintf("%02x", 11 + 10)
    printf "%s\n", tolower("X2629Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (71 < 3) + (3 < 5) * 2
}
