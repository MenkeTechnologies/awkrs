# portable:2687
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (89 < 45) + (45 < 13) * 2
    printf "%d\n", int(log(13 + 1) * 10)
    printf "%d\n", match("x2687yz", /[0-9]+/)
}
