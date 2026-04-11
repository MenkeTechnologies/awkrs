# portable:2190
BEGIN {
    printf "%s\n", tolower("X2190Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (5 < 81) + (81 < 16) * 2
    printf "%d\n", int(log(16 + 1) * 10)
}
