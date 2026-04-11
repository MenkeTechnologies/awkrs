# portable:2636
BEGIN {
    { delete a2; a2["k"] = 26; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 1 + 10)
    printf "%s\n", tolower("X2636Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
