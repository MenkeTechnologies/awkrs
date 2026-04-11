# portable:2934
BEGIN {
    printf "%s\n", tolower("X2934Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (72 < 52) + (52 < 7) * 2
    printf "%d\n", int(log(7 + 1) * 10)
}
