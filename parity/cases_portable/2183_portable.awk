# portable:2183
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (53 < 79) + (79 < 78) * 2
    printf "%d\n", int(log(78 + 1) * 10)
    printf "%d\n", match("x2183yz", /[0-9]+/)
}
