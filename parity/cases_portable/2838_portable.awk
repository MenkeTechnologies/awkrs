# portable:2838
BEGIN {
    printf "%s\n", tolower("X2838Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (79 < 50) + (50 < 51) * 2
    printf "%d\n", int(log(51 + 1) * 10)
}
