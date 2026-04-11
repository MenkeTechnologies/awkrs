# portable:2214
BEGIN {
    printf "%s\n", tolower("X2214Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (76 < 37) + (37 < 5) * 2
    printf "%d\n", int(log(5 + 1) * 10)
}
