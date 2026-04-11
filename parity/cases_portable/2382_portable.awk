# portable:2382
BEGIN {
    printf "%s\n", tolower("X2382Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (88 < 85) + (85 < 11) * 2
    printf "%d\n", int(log(11 + 1) * 10)
}
