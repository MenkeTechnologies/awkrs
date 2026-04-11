# portable:2365
BEGIN {
    printf "%s\n", sprintf("%02x", 2 + 10)
    printf "%s\n", tolower("X2365Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (66 < 42) + (42 < 43) * 2
}
