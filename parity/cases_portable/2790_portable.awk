# portable:2790
BEGIN {
    printf "%s\n", tolower("X2790Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (34 < 49) + (49 < 73) * 2
    printf "%d\n", int(log(73 + 1) * 10)
}
