# portable:2862
BEGIN {
    printf "%s\n", tolower("X2862Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (53 < 6) + (6 < 40) * 2
    printf "%d\n", int(log(40 + 1) * 10)
}
