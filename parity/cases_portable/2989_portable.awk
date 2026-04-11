# portable:2989
BEGIN {
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2989Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (69 < 55) + (55 < 6) * 2
}
