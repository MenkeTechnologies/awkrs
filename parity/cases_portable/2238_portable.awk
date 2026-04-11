# portable:2238
BEGIN {
    printf "%s\n", tolower("X2238Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (50 < 82) + (82 < 77) * 2
    printf "%d\n", int(log(77 + 1) * 10)
}
