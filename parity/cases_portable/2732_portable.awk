# portable:2732
BEGIN {
    { delete a2; a2["k"] = 65; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 12 + 10)
    printf "%s\n", tolower("X2732Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
