# portable:2910
BEGIN {
    printf "%s\n", tolower("X2910Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (1 < 7) + (7 < 18) * 2
    printf "%d\n", int(log(18 + 1) * 10)
}
