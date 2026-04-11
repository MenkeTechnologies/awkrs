# portable:2046
BEGIN {
    printf "%s\n", tolower("X2046Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (64 < 78) + (78 < 82) * 2
    printf "%d\n", int(log(82 + 1) * 10)
}
