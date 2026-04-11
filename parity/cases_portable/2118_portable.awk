# portable:2118
BEGIN {
    printf "%s\n", tolower("X2118Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (83 < 35) + (35 < 49) * 2
    printf "%d\n", int(log(49 + 1) * 10)
}
