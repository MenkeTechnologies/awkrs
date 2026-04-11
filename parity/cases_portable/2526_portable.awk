# portable:2526
BEGIN {
    printf "%s\n", tolower("X2526Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (29 < 88) + (88 < 28) * 2
    printf "%d\n", int(log(28 + 1) * 10)
}
