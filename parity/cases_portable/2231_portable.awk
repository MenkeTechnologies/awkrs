# portable:2231
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (1 < 80) + (80 < 56) * 2
    printf "%d\n", int(log(56 + 1) * 10)
    printf "%d\n", match("x2231yz", /[0-9]+/)
}
