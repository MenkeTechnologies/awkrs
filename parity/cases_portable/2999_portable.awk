# portable:2999
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (42 < 7) + (7 < 36) * 2
    printf "%d\n", int(log(36 + 1) * 10)
    printf "%d\n", match("x2999yz", /[0-9]+/)
}
