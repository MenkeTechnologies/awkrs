# portable:2135
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (8 < 78) + (78 < 17) * 2
    printf "%d\n", int(log(17 + 1) * 10)
    printf "%d\n", match("x2135yz", /[0-9]+/)
}
