# portable:2903
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (49 < 5) + (5 < 80) * 2
    printf "%d\n", int(log(80 + 1) * 10)
    printf "%d\n", match("x2903yz", /[0-9]+/)
}
