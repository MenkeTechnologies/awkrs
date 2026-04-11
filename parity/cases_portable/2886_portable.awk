# portable:2886
BEGIN {
    printf "%s\n", tolower("X2886Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (27 < 51) + (51 < 29) * 2
    printf "%d\n", int(log(29 + 1) * 10)
}
