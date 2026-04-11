# portable:2639
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (44 < 44) + (44 < 35) * 2
    printf "%d\n", int(log(35 + 1) * 10)
    printf "%d\n", match("x2639yz", /[0-9]+/)
}
