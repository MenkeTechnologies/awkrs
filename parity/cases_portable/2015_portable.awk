# portable:2015
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (41 < 31) + (31 < 72) * 2
    printf "%d\n", int(log(72 + 1) * 10)
    printf "%d\n", match("x2015yz", /[0-9]+/)
}
