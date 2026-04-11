# portable:2615
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (70 < 88) + (88 < 46) * 2
    printf "%d\n", int(log(46 + 1) * 10)
    printf "%d\n", match("x2615yz", /[0-9]+/)
}
