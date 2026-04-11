# portable:2550
BEGIN {
    printf "%s\n", tolower("X2550Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (3 < 44) + (44 < 17) * 2
    printf "%d\n", int(log(17 + 1) * 10)
}
