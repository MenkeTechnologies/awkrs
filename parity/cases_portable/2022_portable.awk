# portable:2022
BEGIN {
    printf "%s\n", tolower("X2022Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (90 < 33) + (33 < 10) * 2
    printf "%d\n", int(log(10 + 1) * 10)
}
