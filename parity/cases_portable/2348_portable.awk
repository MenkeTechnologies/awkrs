# portable:2348
BEGIN {
    { delete a2; a2["k"] = 75; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 2 + 10)
    printf "%s\n", tolower("X2348Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
