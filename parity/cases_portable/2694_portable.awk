# portable:2694
BEGIN {
    printf "%s\n", tolower("X2694Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (41 < 47) + (47 < 34) * 2
    printf "%d\n", int(log(34 + 1) * 10)
}
