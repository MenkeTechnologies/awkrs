# portable:2540
BEGIN {
    { delete a2; a2["k"] = 70; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 7 + 10)
    printf "%s\n", tolower("X2540Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
