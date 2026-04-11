# portable:2759
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (11 < 2) + (2 < 63) * 2
    printf "%d\n", int(log(63 + 1) * 10)
    printf "%d\n", match("x2759yz", /[0-9]+/)
}
