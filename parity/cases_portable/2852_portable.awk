# portable:2852
BEGIN {
    { delete a2; a2["k"] = 10; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 13 + 10)
    printf "%s\n", tolower("X2852Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
