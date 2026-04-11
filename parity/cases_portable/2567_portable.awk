# portable:2567
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (25 < 87) + (87 < 68) * 2
    printf "%d\n", int(log(68 + 1) * 10)
    printf "%d\n", match("x2567yz", /[0-9]+/)
}
