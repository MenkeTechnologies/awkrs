# portable:2286
BEGIN {
    printf "%s\n", tolower("X2286Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (95 < 83) + (83 < 55) * 2
    printf "%d\n", int(log(55 + 1) * 10)
}
