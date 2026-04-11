# portable:2958
BEGIN {
    printf "%s\n", tolower("X2958Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (46 < 8) + (8 < 79) * 2
    printf "%d\n", int(log(79 + 1) * 10)
}
