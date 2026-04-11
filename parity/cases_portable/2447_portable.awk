# portable:2447
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (58 < 40) + (40 < 40) * 2
    printf "%d\n", int(log(40 + 1) * 10)
    printf "%d\n", match("x2447yz", /[0-9]+/)
}
