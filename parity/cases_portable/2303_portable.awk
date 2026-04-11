# portable:2303
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (20 < 37) + (37 < 23) * 2
    printf "%d\n", int(log(23 + 1) * 10)
    printf "%d\n", match("x2303yz", /[0-9]+/)
}
