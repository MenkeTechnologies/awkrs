# portable:2718
BEGIN {
    printf "%s\n", tolower("X2718Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (15 < 3) + (3 < 23) * 2
    printf "%d\n", int(log(23 + 1) * 10)
}
