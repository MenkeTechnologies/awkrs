# portable:2646
BEGIN {
    printf "%s\n", tolower("X2646Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (93 < 46) + (46 < 56) * 2
    printf "%d\n", int(log(56 + 1) * 10)
}
