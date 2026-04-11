# portable:2413
BEGIN {
    printf "%s\n", sprintf("%02x", 16 + 10)
    printf "%s\n", tolower("X2413Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (14 < 43) + (43 < 21) * 2
}
