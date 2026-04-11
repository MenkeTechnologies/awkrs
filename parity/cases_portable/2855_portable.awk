# portable:2855
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (4 < 4) + (4 < 19) * 2
    printf "%d\n", int(log(19 + 1) * 10)
    printf "%d\n", match("x2855yz", /[0-9]+/)
}
