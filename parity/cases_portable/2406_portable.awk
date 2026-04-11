# portable:2406
BEGIN {
    printf "%s\n", tolower("X2406Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (62 < 41) + (41 < 83) * 2
    printf "%d\n", int(log(83 + 1) * 10)
}
