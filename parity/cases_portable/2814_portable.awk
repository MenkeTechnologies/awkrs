# portable:2814
BEGIN {
    printf "%s\n", tolower("X2814Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (8 < 5) + (5 < 62) * 2
    printf "%d\n", int(log(62 + 1) * 10)
}
