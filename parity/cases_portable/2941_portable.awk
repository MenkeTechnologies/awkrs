# portable:2941
BEGIN {
    printf "%s\n", sprintf("%02x", 0 + 10)
    printf "%s\n", tolower("X2941Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (24 < 54) + (54 < 28) * 2
}
