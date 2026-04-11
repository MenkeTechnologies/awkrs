# portable:2876
BEGIN {
    { delete a2; a2["k"] = 82; printf "%d\n", a2["k"] }
    printf "%s\n", sprintf("%02x", 3 + 10)
    printf "%s\n", tolower("X2876Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
}
