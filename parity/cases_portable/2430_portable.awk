# portable:2430
BEGIN {
    printf "%s\n", tolower("X2430Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (36 < 86) + (86 < 72) * 2
    printf "%d\n", int(log(72 + 1) * 10)
}
