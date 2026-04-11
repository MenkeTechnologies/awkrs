# portable:2166
BEGIN {
    printf "%s\n", tolower("X2166Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (31 < 36) + (36 < 27) * 2
    printf "%d\n", int(log(27 + 1) * 10)
}
