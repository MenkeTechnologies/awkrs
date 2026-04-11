# portable:2564
BEGIN {
    { delete a2; a2["k"] = 59; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 14 + 10)
    printf "%s\n", tolower("X2564Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
