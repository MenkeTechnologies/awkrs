# portable:2111
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (34 < 33) + (33 < 28) * 2
    printf "%d\n", int(log(28 + 1) * 10)
    printf "%d\n", match("x2111yz", /[0-9]+/)
}
