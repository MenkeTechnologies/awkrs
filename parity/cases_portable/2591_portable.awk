# portable:2591
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (96 < 43) + (43 < 57) * 2
    printf "%d\n", int(log(57 + 1) * 10)
    printf "%d\n", match("x2591yz", /[0-9]+/)
}
