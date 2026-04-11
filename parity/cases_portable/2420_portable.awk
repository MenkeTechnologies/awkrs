# portable:2420
BEGIN {
    { delete a2; a2["k"] = 42; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 6 + 10)
    printf "%s\n", tolower("X2420Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
