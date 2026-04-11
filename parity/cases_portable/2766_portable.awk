# portable:2766
BEGIN {
    printf "%s\n", tolower("X2766Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (60 < 4) + (4 < 84) * 2
    printf "%d\n", int(log(84 + 1) * 10)
}
