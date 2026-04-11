# portable:2142
BEGIN {
    printf "%s\n", tolower("X2142Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (57 < 80) + (80 < 38) * 2
    printf "%d\n", int(log(38 + 1) * 10)
}
