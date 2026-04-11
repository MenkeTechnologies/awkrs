# portable:2924
BEGIN {
    { delete a2; a2["k"] = 60; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 0 + 10)
    printf "%s\n", tolower("X2924Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
