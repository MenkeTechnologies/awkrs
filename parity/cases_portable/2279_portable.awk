# portable:2279
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (46 < 81) + (81 < 34) * 2
    printf "%d\n", int(log(34 + 1) * 10)
    printf "%d\n", match("x2279yz", /[0-9]+/)
}
