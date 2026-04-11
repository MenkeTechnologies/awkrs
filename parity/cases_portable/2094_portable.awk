# portable:2094
BEGIN {
    printf "%s\n", tolower("X2094Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (12 < 79) + (79 < 60) * 2
    printf "%d\n", int(log(60 + 1) * 10)
}
