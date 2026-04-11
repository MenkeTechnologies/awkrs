# portable:2711
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (63 < 90) + (90 < 85) * 2
    printf "%d\n", int(log(85 + 1) * 10)
    printf "%d\n", match("x2711yz", /[0-9]+/)
}
