# portable:2670
BEGIN {
    printf "%s\n", tolower("X2670Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (67 < 2) + (2 < 45) * 2
    printf "%d\n", int(log(45 + 1) * 10)
}
