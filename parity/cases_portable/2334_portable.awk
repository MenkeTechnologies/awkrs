# portable:2334
BEGIN {
    printf "%s\n", tolower("X2334Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (43 < 84) + (84 < 33) * 2
    printf "%d\n", int(log(33 + 1) * 10)
}
