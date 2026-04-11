# portable:2879
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (75 < 49) + (49 < 8) * 2
    printf "%d\n", int(log(8 + 1) * 10)
    printf "%d\n", match("x2879yz", /[0-9]+/)
}
