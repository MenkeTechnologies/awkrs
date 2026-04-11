# portable:2495
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (6 < 41) + (41 < 18) * 2
    printf "%d\n", int(log(18 + 1) * 10)
    printf "%d\n", match("x2495yz", /[0-9]+/)
}
