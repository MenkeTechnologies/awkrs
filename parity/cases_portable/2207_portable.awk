# portable:2207
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (27 < 35) + (35 < 67) * 2
    printf "%d\n", int(log(67 + 1) * 10)
    printf "%d\n", match("x2207yz", /[0-9]+/)
}
