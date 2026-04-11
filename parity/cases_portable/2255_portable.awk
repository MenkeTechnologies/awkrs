# portable:2255
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (72 < 36) + (36 < 45) * 2
    printf "%d\n", int(log(45 + 1) * 10)
    printf "%d\n", match("x2255yz", /[0-9]+/)
}
