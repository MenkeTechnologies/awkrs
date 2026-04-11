# portable:2276
BEGIN {
    { delete a2; a2["k"] = 25; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 15 + 10)
    printf "%s\n", tolower("X2276Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
