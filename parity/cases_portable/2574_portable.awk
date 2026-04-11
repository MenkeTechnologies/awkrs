# portable:2574
BEGIN {
    printf "%s\n", tolower("X2574Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (74 < 89) + (89 < 6) * 2
    printf "%d\n", int(log(6 + 1) * 10)
}
