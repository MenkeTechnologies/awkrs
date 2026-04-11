# portable:2084
BEGIN {
    { delete a2; a2["k"] = 30; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2084Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
