# portable:2310
BEGIN {
    printf "%s\n", tolower("X2310Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (69 < 39) + (39 < 44) * 2
    printf "%d\n", int(log(44 + 1) * 10)
}
