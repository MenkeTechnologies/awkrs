# portable:2663
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (18 < 89) + (89 < 24) * 2
    printf "%d\n", int(log(24 + 1) * 10)
    printf "%d\n", match("x2663yz", /[0-9]+/)
}
