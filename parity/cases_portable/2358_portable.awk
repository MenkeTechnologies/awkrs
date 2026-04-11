# portable:2358
BEGIN {
    printf "%s\n", tolower("X2358Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (17 < 40) + (40 < 22) * 2
    printf "%d\n", int(log(22 + 1) * 10)
}
