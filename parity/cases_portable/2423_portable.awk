# portable:2423
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (84 < 84) + (84 < 51) * 2
    printf "%d\n", int(log(51 + 1) * 10)
    printf "%d\n", match("x2423yz", /[0-9]+/)
}
