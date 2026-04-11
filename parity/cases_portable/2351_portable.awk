# portable:2351
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (65 < 38) + (38 < 84) * 2
    printf "%d\n", int(log(84 + 1) * 10)
    printf "%d\n", match("x2351yz", /[0-9]+/)
}
