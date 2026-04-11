# portable:2543
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (51 < 42) + (42 < 79) * 2
    printf "%d\n", int(log(79 + 1) * 10)
    printf "%d\n", match("x2543yz", /[0-9]+/)
}
