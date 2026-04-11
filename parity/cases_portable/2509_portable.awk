# portable:2509
BEGIN {
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2509Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (7 < 45) + (45 < 60) * 2
}
