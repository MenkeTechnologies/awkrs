# portable:2159
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (79 < 34) + (34 < 6) * 2
    printf "%d\n", int(log(6 + 1) * 10)
    printf "%d\n", match("x2159yz", /[0-9]+/)
}
