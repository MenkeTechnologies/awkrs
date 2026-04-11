# portable:2893
BEGIN {
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2893Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (76 < 53) + (53 < 50) * 2
}
