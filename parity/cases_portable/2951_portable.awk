# portable:2951
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (94 < 6) + (6 < 58) * 2
    printf "%d\n", int(log(58 + 1) * 10)
    printf "%d\n", match("x2951yz", /[0-9]+/)
}
