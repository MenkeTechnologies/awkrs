# portable:2900
BEGIN {
    { delete a2; a2["k"] = 71; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 10 + 10)
    printf "%s\n", tolower("X2900Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
